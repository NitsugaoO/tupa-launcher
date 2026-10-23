//! `actions.toml`：把开关 / 选项 / 数值做成配置驱动。
//!
//! 每个条目是一组命令：
//!
//! - `get` 读当前值（stdout 第一行，去掉首尾空白）
//! - `toggle` 用 `on` / `off` 两个命令写
//! - `choice` / `number` 用 `set`（`{value}` 会替换成实际值）
//!
//! 命令统一走 `cmd /C`，在 worker 线程里执行并带超时；写命令还会把目标值
//! 放到环境变量 `TUPA_VALUE` 里（内置脚本靠它取参数）。
//!
//! 内置了两项**不需要管理员权限**的开关：深色模式、屏幕亮度。
//! 用户 `~/.config/tupa-launcher/actions.toml` 里同 id 的条目会覆盖内置项。

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::config;
use crate::i18n;
use crate::providers::Control;

/// 写命令能读到的环境变量名
pub const ENV_VALUE: &str = "TUPA_VALUE";

/// 单条脚本的最长执行时间
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ActionsFile {
    pub items: Vec<Spec>,
}

/// `actions.toml` 里的一个 `[[items]]`
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Spec {
    /// 稳定的标识（`SetControl` 靠它找到条目）；不写就用标题
    pub id: Option<String>,
    pub title: String,
    pub subtitle: String,
    /// `toggle` / `choice` / `number`
    pub kind: SpecKind,
    /// 分组（`switch 电源` 可以按它过滤）
    pub group: String,
    /// 读取当前值
    pub get: String,
    /// choice / number 的写命令，`{value}` 替换成实际值
    pub set: Option<String>,
    /// toggle 打开时执行的命令
    pub on: Option<String>,
    /// toggle 关闭时执行的命令
    pub off: Option<String>,
    /// `get` 输出这个字符串算「开」（默认接受 1/true/on/开/enabled）
    pub on_value: Option<String>,
    /// choice 的选项
    pub options: Vec<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub step: Option<i64>,
    pub suffix: String,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            id: None,
            title: String::new(),
            subtitle: String::new(),
            kind: SpecKind::Toggle,
            group: String::new(),
            get: String::new(),
            set: None,
            on: None,
            off: None,
            on_value: None,
            options: Vec::new(),
            min: None,
            max: None,
            step: None,
            suffix: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SpecKind {
    #[default]
    Toggle,
    Choice,
    Number,
    /// 只读信息 + 一个动作：`get` 输出显示文本，回车执行 `on`（例如「更新订阅」）
    Action,
}

impl Spec {
    pub fn id(&self) -> String {
        self.id.clone().unwrap_or_else(|| self.title.clone())
    }

    fn number_bounds(&self) -> (i64, i64, i64) {
        (
            self.min.unwrap_or(0),
            self.max.unwrap_or(100),
            self.step.unwrap_or(1),
        )
    }
}

/// 从磁盘读用户配置（不存在就只返回内置项）
pub fn load() -> Vec<Spec> {
    let mut list = builtin();
    let Some(path) = path() else {
        return list;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return list;
    };
    match toml::from_str::<ActionsFile>(&text) {
        Ok(file) => merge(&mut list, file.items),
        Err(error) => {
            eprintln!("解析 {} 失败：{error:#}", path.display());
        }
    }
    list
}

fn path() -> Option<std::path::PathBuf> {
    Some(config::config_dir()?.join("actions.toml"))
}

/// 用户条目覆盖同 id 的内置条目（顺序上用户条目排在后面）
fn merge(list: &mut Vec<Spec>, user: Vec<Spec>) {
    let ids: Vec<String> = user.iter().map(Spec::id).collect();
    list.retain(|spec| !ids.contains(&spec.id()));
    list.extend(user);
}

/// 读一个条目当前的状态（失败 = [`Control::Unavailable`]）
pub fn get_control(spec: &Spec) -> Control {
    let Ok(output) = run_script(&spec.get, None) else {
        return Control::Unavailable;
    };
    let raw = output.lines().next().unwrap_or("").trim().to_string();

    match spec.kind {
        SpecKind::Toggle => Control::Toggle {
            value: parse_on_value(&raw, spec.on_value.as_deref()),
        },
        SpecKind::Choice => {
            if spec.options.is_empty() {
                return Control::Unavailable;
            }
            let index = spec
                .options
                .iter()
                .position(|option| option.eq_ignore_ascii_case(&raw))
                .unwrap_or(0);
            Control::Choice {
                options: spec.options.clone(),
                index,
            }
        }
        SpecKind::Action => Control::Action { detail: raw },
        SpecKind::Number => {
            let Ok(value) = raw.parse::<i64>() else {
                return Control::Unavailable;
            };
            let (min, max, step) = spec.number_bounds();
            Control::Number {
                value: value.clamp(min, max),
                min,
                max,
                step: step.max(1),
                suffix: spec.suffix.clone(),
            }
        }
    }
}

/// 把某个条目设成新值。`value` 对 toggle 是 `"on"` / `"off"`，
/// 对 choice 是选项文本，对 number 是数字字符串。
pub fn apply(id: &str, value: &str) -> Result<String> {
    let Some(spec) = load().into_iter().find(|spec| spec.id() == id) else {
        bail!("{}", i18n::action_not_found(id));
    };

    let command = match spec.kind {
        SpecKind::Toggle => {
            let on = parse_on(value);
            let picked = if on {
                spec.on.as_ref().or(spec.set.as_ref())
            } else {
                spec.off.as_ref().or(spec.set.as_ref())
            };
            picked.cloned()
        }
        SpecKind::Choice | SpecKind::Number => spec.set.as_ref().map(|set| substitute(set, value)),
        SpecKind::Action => spec.on.as_ref().or(spec.set.as_ref()).cloned(),
    };

    let Some(command) = command else {
        bail!("{}", i18n::action_no_set(&spec.title));
    };

    let output = run_script(&command, Some(value))?;
    // 动作类条目直接用脚本最后一行当提示（例如「已更新并重启 Clash Party」）
    if spec.kind == SpecKind::Action {
        let message = output.lines().last().unwrap_or("").trim().to_string();
        return Ok(if message.is_empty() {
            i18n::control_applied(&spec.title, "")
        } else {
            message
        });
    }
    Ok(i18n::control_applied(&spec.title, &spec.kind.label(value)))
}

impl SpecKind {
    /// 写完成之后状态栏里的值文本
    fn label(self, value: &str) -> String {
        match self {
            SpecKind::Action => String::new(),
            SpecKind::Toggle => {
                if parse_on(value) {
                    i18n::control_on().to_string()
                } else {
                    i18n::control_off().to_string()
                }
            }
            SpecKind::Choice | SpecKind::Number => value.to_string(),
        }
    }
}

/// `get` 输出 / 写命令的参数是否表示「开」
pub fn parse_on_value(raw: &str, on_value: Option<&str>) -> bool {
    if let Some(on_value) = on_value {
        return raw.eq_ignore_ascii_case(on_value.trim());
    }
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes" | "enabled" | "开" | "开启" | "启用"
    )
}

fn parse_on(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes" | "开" | "开启" | "启用"
    )
}

fn substitute(template: &str, value: &str) -> String {
    template.replace("{value}", value)
}

/// 跑一条命令，返回 stdout（已 trim）。失败 / 超时会带第一行 stderr。
/// 从 `powershell ... -EncodedCommand <base64>` 里取出 base64；不是这个格式就返回 None。
fn encoded_command(command: &str) -> Option<&str> {
    let (_, encoded) = command.rsplit_once(" -EncodedCommand ")?;
    if encoded.is_empty()
        || !encoded
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
    {
        return None;
    }
    Some(encoded)
}

pub fn run_script(command: &str, value: Option<&str>) -> Result<String> {
    if command.trim().is_empty() {
        bail!("{}", i18n::action_empty_command());
    }

    // 内置 PowerShell 用 `-EncodedCommand`：直接启动 powershell，绕开 `cmd /C`
    // 的 8191 字符命令行限制（长脚本的 base64 很容易超）。用户命令仍然走 cmd。
    let mut cmd = if let Some(encoded) = encoded_command(command) {
        let mut cmd = Command::new("powershell.exe");
        cmd.arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-EncodedCommand")
            .arg(encoded);
        cmd
    } else {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/C").arg(command);
        cmd
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(value) = value {
        cmd.env(ENV_VALUE, value);
    }
    crate::util::hide_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .with_context(|| i18n::cannot("执行", "run", command))?;

    // 管道必须同时读：输出多了会写满管道把子进程卡住
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut stdout) = stdout {
            use std::io::Read;
            let _ = stdout.read_to_end(&mut buffer);
        }
        buffer
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut stderr) = stderr {
            use std::io::Read;
            let _ = stderr.read_to_end(&mut buffer);
        }
        buffer
    });

    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).with_context(|| i18n::cannot("执行", "run", command));
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{}", i18n::script_timeout(command, TIMEOUT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(30));
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    if !status.success() {
        let text = crate::util::decode_output(&stderr);
        let first = text.lines().next().unwrap_or("").trim();
        bail!("{}", i18n::script_failed(command, first));
    }

    Ok(crate::util::decode_output(&stdout).trim().to_string())
}

// ---------------------------------------------------------------------------
// 内置条目（都不需要管理员权限）
// ---------------------------------------------------------------------------

fn builtin() -> Vec<Spec> {
    let battery = |body: &str| powershell(&format!("{BATTERY_BOOTSTRAP}{body}"));

    vec![
        Spec {
            id: Some("dark-mode".to_string()),
            title: i18n::action_dark_mode().to_string(),
            subtitle: i18n::action_appearance().to_string(),
            kind: SpecKind::Toggle,
            group: i18n::action_group_appearance().to_string(),
            get: powershell(DARK_GET),
            on: Some(powershell(DARK_ON)),
            off: Some(powershell(DARK_OFF)),
            on_value: Some("0".to_string()),
            ..Spec::default()
        },
        Spec {
            id: Some("battery-care".to_string()),
            title: i18n::action_battery_care().to_string(),
            subtitle: "Lenovo Vantage".to_string(),
            kind: SpecKind::Toggle,
            group: i18n::action_group_power().to_string(),
            get: battery(BATTERY_GET_BODY),
            on: Some(battery(BATTERY_ON_BODY)),
            off: Some(battery(BATTERY_OFF_BODY)),
            on_value: Some("Storage".to_string()),
            ..Spec::default()
        },
        Spec {
            id: Some("energy-saver".to_string()),
            // Lenovo 的「静音省电」和 Windows 的 Energy Saver 合成一个开关：
            // 打开 = 两个都开，关闭 = Vantage 回智能模式 + Energy Saver 关掉
            title: i18n::action_energy_saver().to_string(),
            subtitle: "Lenovo Vantage + Windows".to_string(),
            kind: SpecKind::Toggle,
            group: i18n::action_group_power().to_string(),
            get: powershell(&format!(
                "{VANTAGE_POWER_BOOTSTRAP}{ENERGY_SAVER_GET_COMBINED}"
            )),
            on: Some(powershell(&format!(
                "{VANTAGE_POWER_BOOTSTRAP}{ENERGY_SAVER_BOOTSTRAP}{ENERGY_SAVER_VANTAGE_ON}{ENERGY_SAVER_ON_BODY}"
            ))),
            off: Some(powershell(&format!(
                "{VANTAGE_POWER_BOOTSTRAP}{ENERGY_SAVER_BOOTSTRAP}{ENERGY_SAVER_VANTAGE_OFF}{ENERGY_SAVER_OFF_BODY}"
            ))),
            on_value: Some("on".to_string()),
            ..Spec::default()
        },
        Spec {
            id: Some("keep-awake".to_string()),
            title: i18n::action_keep_awake().to_string(),
            subtitle: "nosleep".to_string(),
            kind: SpecKind::Toggle,
            group: i18n::action_group_power().to_string(),
            get: powershell(KEEP_AWAKE_GET),
            on: Some(powershell(KEEP_AWAKE_ON)),
            off: Some(powershell(KEEP_AWAKE_OFF)),
            on_value: Some("on".to_string()),
            ..Spec::default()
        },
        Spec {
            id: Some("clash-subscription".to_string()),
            title: i18n::action_clash_subscription().to_string(),
            subtitle: "Clash Party".to_string(),
            kind: SpecKind::Action,
            group: i18n::action_group_network().to_string(),
            get: powershell(CLASH_INFO_GET),
            on: Some(powershell(CLASH_UPDATE)),
            ..Spec::default()
        },
        Spec {
            id: Some("brightness".to_string()),
            title: i18n::action_brightness().to_string(),
            subtitle: i18n::action_display().to_string(),
            kind: SpecKind::Number,
            group: i18n::action_group_display().to_string(),
            get: powershell(BRIGHTNESS_GET),
            set: Some(powershell(BRIGHTNESS_SET)),
            min: Some(0),
            max: Some(100),
            step: Some(10),
            suffix: "%".to_string(),
            ..Spec::default()
        },
    ]
}

fn powershell(script: &str) -> String {
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        base64_utf16le(script)
    )
}

/// PowerShell 的 `-EncodedCommand` 要求 UTF-16LE + base64。
/// 自己编码可以绕开 `cmd /C` 的引号 / 管道转义地狱。
fn base64_utf16le(text: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = text
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

const DARK_GET: &str = r#"
$p = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' -ErrorAction Stop
$p.AppsUseLightTheme
"#;

const DARK_ON: &str = r#"
$p = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
Set-ItemProperty $p -Name AppsUseLightTheme -Value 0 -ErrorAction Stop
Set-ItemProperty $p -Name SystemUsesLightTheme -Value 0 -ErrorAction Stop
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TupaTheme {
    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
    public static void Notify() {
        UIntPtr result;
        SendMessageTimeout(new IntPtr(0xffff), 0x001A, UIntPtr.Zero, "ImmersiveColorSet", 2, 1000, out result);
    }
}
'@
[TupaTheme]::Notify()
"#;

const DARK_OFF: &str = r#"
$p = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
Set-ItemProperty $p -Name AppsUseLightTheme -Value 1 -ErrorAction Stop
Set-ItemProperty $p -Name SystemUsesLightTheme -Value 1 -ErrorAction Stop
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TupaTheme {
    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
    public static void Notify() {
        UIntPtr result;
        SendMessageTimeout(new IntPtr(0xffff), 0x001A, UIntPtr.Zero, "ImmersiveColorSet", 2, 1000, out result);
    }
}
'@
[TupaTheme]::Notify()
"#;

const BRIGHTNESS_GET: &str = r#"
$b = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness -ErrorAction Stop | Select-Object -First 1
if (-not $b) { exit 3 }
$b.CurrentBrightness
"#;

const BRIGHTNESS_SET: &str = r#"
$v = [byte][int]$env:TUPA_VALUE
$m = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods -ErrorAction Stop | Select-Object -First 1
if (-not $m) { exit 3 }
$null = Invoke-CimMethod -InputObject $m -MethodName WmiSetBrightness -Arguments @{ Timeout = [uint32]1; Brightness = $v } -ErrorAction Stop
"#;

// ---------------------------------------------------------------------------
// 电池养护模式：直接调 Vantage 的 IdeaBatteryAgent.dll（不需要管理员）
//
// ThinkBook / IdeaPad 的养护模式在 Windows 下只由 Vantage 驱动控制，
// WMI（root\wmi）和 BIOS 设置里都没有。Vantage 的 Addin 进程已经在跑，
// 它的托管程序集提供了 Get/SetBatteryChargeMode，反射调用即可。
// ---------------------------------------------------------------------------

const BATTERY_BOOTSTRAP: &str = r#"
$ErrorActionPreference = 'Stop'
$root = Join-Path $env:ProgramData 'Lenovo\Vantage\Addins\IdeaNotebookAddin'
$dir = Get-ChildItem -LiteralPath $root -Directory -ErrorAction Stop |
    Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } -Descending |
    Select-Object -First 1
if (-not $dir) { throw 'IdeaNotebookAddin not installed' }
$battery = Join-Path $dir.FullName 'IdeaBatteryAgent.dll'
if (-not (Test-Path -LiteralPath $battery)) { throw 'IdeaBatteryAgent.dll not found' }
foreach ($name in 'Newtonsoft.Json.dll','Lenovo.Vantage.RpcCommon.dll','Lenovo.Vantage.RpcClient.dll','Lenovo.VantageService.Utilities.dll','Lenovo.VantageService.UtilitiesExtensions.dll','BatteryManagementContract.dll','BatteryInformationContract.dll','PowerContract.dll') {
    $path = Join-Path $dir.FullName $name
    if (Test-Path -LiteralPath $path) { try { [Reflection.Assembly]::LoadFrom($path) | Out-Null } catch {} }
}
$assembly = [Reflection.Assembly]::LoadFrom($battery)
$type = $assembly.GetType('IdeaNotebookAddin.BatteryAgent')
$agent = $type.GetMethod('GetInstance', [Reflection.BindingFlags]'Public,Static,Instance').Invoke($null, @())
"#;

const BATTERY_GET_BODY: &str = r#"
$response = $type.GetMethod('GetBatteryChargeMode').Invoke($agent, @())
$response.BatteryChargeMode
"#;

const BATTERY_ON_BODY: &str = r#"
$request = [Activator]::CreateInstance([Lenovo.Modern.Contracts.BatteryManagement.BatteryMgmtRequest])
$request.BatteryChargeMode = [Lenovo.Modern.Contracts.BatteryManagement.BatteryChargeModeType]::Storage
$response = $type.GetMethod('SetBatteryChargeMode').Invoke($agent, @($request))
$response.BatteryChargeMode
"#;

const BATTERY_OFF_BODY: &str = r#"
$request = [Activator]::CreateInstance([Lenovo.Modern.Contracts.BatteryManagement.BatteryMgmtRequest])
$request.BatteryChargeMode = [Lenovo.Modern.Contracts.BatteryManagement.BatteryChargeModeType]::Normal
$response = $type.GetMethod('SetBatteryChargeMode').Invoke($agent, @($request))
$response.BatteryChargeMode
"#;

// ---------------------------------------------------------------------------
// Lenovo Vantage 电源模式（IdeaPowerAgent.dll，普通用户权限）
//
// Windows 侧没有可编程接口，只有 Vantage 的托管 Addin 暴露了
// GetITSMode / SetITSMode（见 docs/08-vantage-power-mode.md）。
// ---------------------------------------------------------------------------

const VANTAGE_POWER_BOOTSTRAP: &str = r#"
$ErrorActionPreference = 'Stop'
$vantageRoot = Join-Path $env:ProgramData 'Lenovo\Vantage\Addins\IdeaNotebookAddin'
$vantageDir = $null
$vantageType = $null
$vantageAgent = $null
try {
    $vantageDir = Get-ChildItem -LiteralPath $vantageRoot -Directory -ErrorAction Stop |
        Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } -Descending |
        Select-Object -First 1
} catch { $vantageDir = $null }
if ($vantageDir) {
    foreach ($name in 'Newtonsoft.Json.dll','Lenovo.Vantage.RpcCommon.dll','Lenovo.Vantage.RpcClient.dll','Lenovo.VantageService.Utilities.dll','Lenovo.VantageService.UtilitiesExtensions.dll','PowerContract.dll') {
        $path = Join-Path $vantageDir.FullName $name
        if (Test-Path -LiteralPath $path) { try { [Reflection.Assembly]::LoadFrom($path) | Out-Null } catch {} }
    }
    $vantageAssembly = [Reflection.Assembly]::LoadFrom((Join-Path $vantageDir.FullName 'IdeaPowerAgent.dll'))
    $vantageType = $vantageAssembly.GetType('IdeaNotebookAddin.PowerAgent')
    $vantageAgent = $vantageType.GetMethod('GetInstance', [Reflection.BindingFlags]'Public,Static,Instance').Invoke($null, @())
}
function Read-VantageMode {
    if (-not $vantageType) { return $null }
    $request = [Activator]::CreateInstance([Lenovo.Modern.Contracts.Power.PowerSettingsRequest])
    $response = $vantageType.GetMethod('GetITSMode').Invoke($vantageAgent, @($request))
    return [string]$response.ItsMode
}
function Write-VantageMode([string]$mode) {
    if (-not $vantageType) { return }
    $request = [Activator]::CreateInstance([Lenovo.Modern.Contracts.Power.PowerSettingsRequest])
    $request.ItsMode = [Lenovo.Modern.Contracts.Power.ItsModeType]$mode
    $null = $vantageType.GetMethod('SetITSMode').Invoke($vantageAgent, @($request))
}
"#;

// ---------------------------------------------------------------------------
// 节能模式 = Lenovo Vantage 的「静音省电」+ Windows 11 Energy Saver
//
// 没有公开的即时 API：注册表 EnergySaverState 要重启，WNF 写入被 ACL 挡住
// （SYSTEM 写在 Session 0，跟用户会话隔离）。所以走快速设置 UI 点击：
//   Win+A -> 点击「节能模式」按钮 -> Win+A 关闭
// 坐标是相对屏幕右下角的逻辑像素偏移，按 DPI 缩放；当前 3072x1920 @200%
// 下按钮中心为 (2894, 1298)。如果快速设置的布局/固定项变了，改这两个数。
// 点击后读注册表校验，没变就报错，不会静默成功。
// ---------------------------------------------------------------------------

const ENERGY_SAVER_BOOTSTRAP: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class TupaEnergy {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
    public static void WinA() {
        keybd_event(0x5B,0,0,UIntPtr.Zero); keybd_event(0x41,0,0,UIntPtr.Zero);
        keybd_event(0x41,0,2,UIntPtr.Zero); keybd_event(0x5B,0,2,UIntPtr.Zero);
    }
    public static string FgClass() {
        var sb = new StringBuilder(256);
        GetClassName(GetForegroundWindow(), sb, 256);
        return sb.ToString();
    }
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(120);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }
    public static void Tap(byte vk) {
        keybd_event(vk, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(20);
        keybd_event(vk, 0, 2, UIntPtr.Zero);
        System.Threading.Thread.Sleep(30);
    }
}
'@
[TupaEnergy]::SetProcessDPIAware() | Out-Null
$key = 'HKLM:\SYSTEM\CurrentControlSet\Control\Power'
function Get-State { (Get-ItemProperty $key -Name EnergySaverState).EnergySaverState }
function Is-Open { [TupaEnergy]::FgClass() -eq 'ControlCenterWindow' }
function Wait-Open {
    for ($i = 0; $i -lt 40; $i++) { if (Is-Open) { return $true }; Start-Sleep -Milliseconds 50 }
    return $false
}
function Wait-State([int]$want) {
    for ($i = 0; $i -lt 30; $i++) { if ((Get-State) -eq $want) { return $true }; Start-Sleep -Milliseconds 50 }
    return $false
}
"#;

/// 合并后的状态：Vantage 是「静音省电」**并且** Windows Energy Saver 打开才算开
const ENERGY_SAVER_GET_COMBINED: &str = r#"
$state = 0
try { $state = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Power' -Name EnergySaverState).EnergySaverState } catch {}
$mode = $null
try { $mode = Read-VantageMode } catch {}
$vantageOk = (-not $mode) -or ($mode -eq 'MmcCool')
if ($vantageOk -and ($state -eq 1)) { 'on' } else { 'off' }
"#;

/// 切 Vantage 的部分（Energy Saver 那段复用原来的 ON/OFF body）
const ENERGY_SAVER_VANTAGE_ON: &str = r#"
$mode = $null
try { $mode = Read-VantageMode } catch {}
if ($mode -and $mode -ne 'MmcCool') { Write-VantageMode 'MmcCool' }
"#;

const ENERGY_SAVER_VANTAGE_OFF: &str = r#"
$mode = $null
try { $mode = Read-VantageMode } catch {}
if ($mode -and $mode -ne 'ItsAuto') { Write-VantageMode 'ItsAuto' }
"#;

const ENERGY_SAVER_ON_BODY: &str = r#"
$want = 1
if ((Get-State) -eq $want) { 'on'; exit 0 }
if (-not (Is-Open)) { [TupaEnergy]::WinA(); if (-not (Wait-Open)) { throw 'quick settings did not open' } }
try {
    # 方向键：Home 回第一个按钮，再 Right x2 + Down 到「节能模式」，Space 切换
    [TupaEnergy]::Tap(0x24)
    [TupaEnergy]::Tap(0x27)
    [TupaEnergy]::Tap(0x27)
    [TupaEnergy]::Tap(0x28)
    [TupaEnergy]::Tap(0x20)
    if (-not (Wait-State $want)) {
        # 回退：鼠标点击（布局与预期不同时）
        $scale = [TupaEnergy]::GetDpiForSystem() / 96.0
        $x = [int]([TupaEnergy]::GetSystemMetrics(0) - 89 * $scale)
        $y = [int]([TupaEnergy]::GetSystemMetrics(1) - 311 * $scale)
        [TupaEnergy]::Click($x, $y)
        if (-not (Wait-State $want)) { throw 'energy saver did not change (quick settings layout changed?)' }
    }
} finally {
    if (Is-Open) { [TupaEnergy]::WinA() }
}
'on'
"#;

const ENERGY_SAVER_OFF_BODY: &str = r#"
$want = 2
if ((Get-State) -eq $want) { 'off'; exit 0 }
if (-not (Is-Open)) { [TupaEnergy]::WinA(); if (-not (Wait-Open)) { throw 'quick settings did not open' } }
try {
    [TupaEnergy]::Tap(0x24)
    [TupaEnergy]::Tap(0x27)
    [TupaEnergy]::Tap(0x27)
    [TupaEnergy]::Tap(0x28)
    [TupaEnergy]::Tap(0x20)
    if (-not (Wait-State $want)) {
        $scale = [TupaEnergy]::GetDpiForSystem() / 96.0
        $x = [int]([TupaEnergy]::GetSystemMetrics(0) - 89 * $scale)
        $y = [int]([TupaEnergy]::GetSystemMetrics(1) - 311 * $scale)
        [TupaEnergy]::Click($x, $y)
        if (-not (Wait-State $want)) { throw 'energy saver did not change (quick settings layout changed?)' }
    }
} finally {
    if (Is-Open) { [TupaEnergy]::WinA() }
}
'off'
"#;

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Clash Party 订阅：读 profile.yaml 的当前订阅 + 上次更新时间，回车触发更新
//
// 更新流程：下载订阅 → 覆盖 profiles/<id>.yaml → 改 profile.yaml 的 updated
// → 重启 Clash Party。只有它自己会重新生成 work/config.yaml，
// 单靠内核 API reload 会丢掉端口 / TUN / 规则集等它合并出来的设置。
// ---------------------------------------------------------------------------

const CLASH_INFO_GET: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$appDir = Join-Path $env:APPDATA 'mihomo-party'
$profileYaml = Join-Path $appDir 'profile.yaml'
if (-not (Test-Path -LiteralPath $profileYaml)) { throw 'mihomo-party profile.yaml not found' }
$current = ''
$items = @{}
$id = ''
foreach ($line in Get-Content -LiteralPath $profileYaml -Encoding UTF8) {
    if ($line -match '^\s*-\s*id:\s*(\S+)') { $id = $matches[1]; $items[$id] = @{ name = ''; updated = 0 } }
    elseif ($id -and $line -match '^\s+name:\s*(.+)$') { $items[$id].name = $matches[1].Trim() }
    elseif ($id -and $line -match '^\s+updated:\s*(\d+)') { $items[$id].updated = [int64]$matches[1] }
    elseif ($line -match '^current:\s*(\S+)') { $current = $matches[1] }
}
if (-not $current -or -not $items.ContainsKey($current)) { throw 'no current profile' }
$item = $items[$current]
$name = if ($item.name) { $item.name } else { $current }
if ($item.updated -le 0) { Write-Output "$name · 从未更新"; exit 0 }
$span = (Get-Date) - [DateTimeOffset]::FromUnixTimeMilliseconds($item.updated).LocalDateTime
$age = if ($span.TotalMinutes -lt 1) { '刚刚' }
    elseif ($span.TotalHours -lt 1) { '{0:N0} 分钟前' -f $span.TotalMinutes }
    elseif ($span.TotalDays -lt 1) { '{0:N0} 小时前' -f $span.TotalHours }
    else { '{0:N0} 天前' -f $span.TotalDays }
Write-Output "$name · $age"
"#;

const CLASH_UPDATE: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$appDir = Join-Path $env:APPDATA 'mihomo-party'
$profileYaml = Join-Path $appDir 'profile.yaml'
if (-not (Test-Path -LiteralPath $profileYaml)) { throw 'mihomo-party profile.yaml not found' }

$current = ''
$items = @{}
$id = ''
foreach ($line in Get-Content -LiteralPath $profileYaml -Encoding UTF8) {
    if ($line -match '^\s*-\s*id:\s*(\S+)') { $id = $matches[1]; $items[$id] = @{ name = ''; url = ''; updated = 0 } }
    elseif ($id -and $line -match '^\s+name:\s*(.+)$') { $items[$id].name = $matches[1].Trim() }
    elseif ($id -and $line -match '^\s+url:\s*(\S+)') { $items[$id].url = $matches[1].Trim() }
    elseif ($id -and $line -match '^\s+updated:\s*(\d+)') { $items[$id].updated = [int64]$matches[1] }
    elseif ($line -match '^current:\s*(\S+)') { $current = $matches[1] }
}
if (-not $current -or -not $items.ContainsKey($current)) { throw 'no current profile' }
$item = $items[$current]
if (-not $item.url) { throw '当前订阅没有 URL（本地配置无法在线更新）' }

$response = Invoke-WebRequest -Uri $item.url -UseBasicParsing -TimeoutSec 30 -Headers @{
    'User-Agent' = 'clash.meta/mihomo-party'
}
$content = $response.Content
if ($content -notmatch '(?m)^(proxies|proxy-providers):') {
    throw '下载到的内容不像 Clash 配置（缺少 proxies / proxy-providers）'
}

$profileFile = Join-Path $appDir ("profiles\$current.yaml")
if (Test-Path -LiteralPath $profileFile) { Copy-Item -LiteralPath $profileFile -Destination "$profileFile.bak" -Force }
[IO.File]::WriteAllText($profileFile, $content, (New-Object Text.UTF8Encoding($false)))

$lines = Get-Content -LiteralPath $profileYaml -Encoding UTF8
$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$inCurrent = $false
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^\s*-\s*id:\s*(\S+)') {
        $inCurrent = ($matches[1] -eq $current)
    } elseif ($inCurrent -and $lines[$i] -match '^(\s*)updated:') {
        $lines[$i] = $matches[1] + 'updated: ' + $now
        $inCurrent = $false
    }
}
[IO.File]::WriteAllLines($profileYaml, $lines, (New-Object Text.UTF8Encoding($false)))

$exe = $null
try {
    $command = (Get-ItemProperty 'HKCU:\Software\Classes\clash\shell\open\command' -ErrorAction Stop).'(default)'
    $exe = ($command -split '"')[1]
} catch {}
if (-not $exe -or -not (Test-Path -LiteralPath $exe)) {
    $exe = 'C:\Program Files\Clash Party\Clash Party.exe'
}
Get-Process -Name 'Clash Party' -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800
Start-Process -FilePath $exe
Start-Sleep -Seconds 2
Write-Output '已更新并重启 Clash Party'
"#;

// 保持唤醒：nosleep（D:\Softwares\nosleep\nosleep.exe）
// ---------------------------------------------------------------------------

const KEEP_AWAKE_GET: &str = r#"
$ErrorActionPreference = 'Stop'
if (Get-Process nosleep -ErrorAction SilentlyContinue) { 'on' } else { 'off' }
"#;

const KEEP_AWAKE_ON: &str = r#"
$ErrorActionPreference = 'Stop'
if (-not (Get-Process nosleep -ErrorAction SilentlyContinue)) {
    Start-Process 'D:\Softwares\nosleep\nosleep.exe' -ArgumentList '--tray','--startup'
    Start-Sleep -Milliseconds 800
}
if (-not (Get-Process nosleep -ErrorAction SilentlyContinue)) { throw 'nosleep did not start' }
'on'
"#;

const KEEP_AWAKE_OFF: &str = r#"
$ErrorActionPreference = 'Stop'
Get-Process nosleep -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
if (Get-Process nosleep -ErrorAction SilentlyContinue) { throw 'nosleep is still running' }
'off'
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_file() {
        let text = r#"
[[items]]
title = "电池养护"
kind = "toggle"
group = "电源"
get = "echo 1"
on = "echo on"
off = "echo off"
on_value = "1"

[[items]]
title = "性能模式"
kind = "choice"
options = ["安静", "均衡", "性能"]
get = "echo 均衡"
set = "perf {value}"
"#;
        let file: ActionsFile = toml::from_str(text).expect("应该能解析");
        assert_eq!(file.items.len(), 2);
        assert_eq!(file.items[0].kind, SpecKind::Toggle);
        assert_eq!(file.items[0].id(), "电池养护");
        assert_eq!(file.items[1].kind, SpecKind::Choice);
        assert_eq!(file.items[1].options.len(), 3);
    }

    #[test]
    fn on_value_has_sensible_defaults() {
        for raw in ["1", "true", "ON", "yes", "开", " enabled "] {
            assert!(parse_on_value(raw, None), "{raw} 应该算开");
        }
        for raw in ["0", "false", "off", "关", ""] {
            assert!(!parse_on_value(raw, None), "{raw} 应该算关");
        }

        // 指定 on_value 之后只看它
        assert!(parse_on_value("dark", Some("dark")));
        assert!(!parse_on_value("1", Some("dark")));
    }

    #[test]
    fn substitutes_value_placeholder() {
        assert_eq!(substitute("perf {value}", "性能"), "perf 性能");
        assert_eq!(substitute("no placeholder", "x"), "no placeholder");
    }

    #[test]
    fn base64_matches_known_vector() {
        // PowerShell 的 -EncodedCommand 编码是 UTF-16LE
        assert_eq!(base64_utf16le("A"), "QQA=");
        assert_eq!(base64_utf16le("AB"), "QQBCAA==");
        assert_eq!(base64_utf16le(""), "");
    }

    #[test]
    fn user_specs_override_builtin_by_id() {
        let mut list = vec![
            Spec {
                id: Some("dark-mode".to_string()),
                title: "内置".to_string(),
                ..Spec::default()
            },
            Spec {
                id: Some("other".to_string()),
                title: "别的".to_string(),
                ..Spec::default()
            },
        ];
        merge(
            &mut list,
            vec![Spec {
                id: Some("dark-mode".to_string()),
                title: "用户".to_string(),
                ..Spec::default()
            }],
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].title, "别的");
        assert_eq!(list[1].title, "用户");
    }
}
