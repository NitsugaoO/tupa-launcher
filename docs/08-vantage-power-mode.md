# Lenovo Vantage 电源模式（`IdeaPowerAgent.SetITSMode`）调研与实现

> 目标：在不弹 UAC、不装驱动、不逆向 RPC 的前提下，读写 Lenovo Vantage 的
> Smart Power Pro 模式（智能 / 静音省电 / 性能 / 极客）。
>
> 结论：**直接反射 Vantage Addin 的 `IdeaPowerAgent.dll`**，普通用户权限即可。
> 本文是自包含的，另一个对话不需要再翻其他文档。
>
> **改名注记（2026-10）**：文中的 `:switch` 现已改名为 `:set`（并新增了 `clash-subscription`
> 「更新订阅」等项）。本文是调研与设计记录，正文不改历史；当前命令与现状见 [`../STATE.md`](../STATE.md)。

## 0. TL;DR

- 机器：ThinkBook 14 G8+（21VG），Windows 11 25H2
- Vantage Addin 版本示例：`1.0.13.114`（路径带版本号，**必须动态查找**）
- 关键 DLL：`C:\ProgramData\Lenovo\Vantage\Addins\IdeaNotebookAddin\<版本>\IdeaPowerAgent.dll`
- 类型：`IdeaNotebookAddin.PowerAgent`
- 读：`GetITSMode(PowerSettingsRequest)` → `PowerSettingsResponse.ItsMode`
- 写：`SetITSMode(PowerSettingsRequest{ ItsMode = ... })`
- 枚举 `Lenovo.Modern.Contracts.Power.ItsModeType`：
  `None=0`, `ItsAuto=1`（智能）, `MmcCool=2`（静音省电）, `MmcPerformance=3`（性能）,
  `MmcGeek=4`（极客）, `MmcCustomGeek=5`
- **注意**：智能模式是 `ItsAuto`，不是 `MmcAuto`（`MmcAuto` 是另一个枚举
  `SupportedMmcModeType` 的成员，值 1，用于位标志）。
- 权限：普通用户，无需管理员/UAC。

## 1. 调研过的失败路径（别重复踩）

目标机器上先试了 WMI / BIOS，全部不可行：

| 路径 | 结果 |
|---|---|
| `root\wmi` 的 `LENOVO_GAMEZONE_DATA` | 普通用户 `Get-CimInstance` → 拒绝访问 `0x80041003`；管理员下能拿实例，但所有 `Get*`/`Set*` 方法报「无效的对象」（ThinkBook 不是 Gaming 系列，方法未实现） |
| `Lenovo_BiosSetting` | 没有 `BatteryChargeMode` / 电源模式相关项 |
| `Lenovo_GetBiosSelections("PowerChargeMode")` | `ret=True` 但 `Selections` 为空（不支持） |
| `LENOVO_UTILITY_DATA.GetIfSupportOrVersion(0..64)` | 只返回版本号，没有开关语义 |
| `LENOVO_OTHER_METHOD.GetFeatureValue` | 只有部分 ID 可读，没有 `0x03010001`（充电限制）/电源模式 |
| 24H2 Energy Saver 的 WNF | 读可以，写被 ACL 拒绝（普通/管理员）；SYSTEM 写成功但 Session 0 与用户会话隔离 |

结论：**联想电源模式走 Vantage 的托管 Addin**，不是 WMI。

## 2. 最终路径：反射 Vantage Addin

Vantage 的 `IdeaNotebookAddin` 目录里有一组 .NET / C++/CLI 程序集：

```
C:\ProgramData\Lenovo\Vantage\Addins\IdeaNotebookAddin\<版本>\
├── IdeaPowerAgent.dll                 # IdeaNotebookAddin.PowerAgent（电源模式）
├── IdeaBatteryAgent.dll               # IdeaNotebookAddin.BatteryAgent（电池养护）
├── PowerContract.dll                  # Lenovo.Modern.Contracts.Power.*
├── BatteryManagementContract.dll
├── BatteryInformationContract.dll
├── Lenovo.Vantage.RpcCommon.dll
├── Lenovo.Vantage.RpcClient.dll
├── Lenovo.VantageService.Utilities.dll
├── Lenovo.VantageService.UtilitiesExtensions.dll
└── Newtonsoft.Json.dll
```

`PowerAgent` 暴露的方法（反射得到）：

```csharp
IdeaNotebookAddin.PowerAgent.GetInstance()
  .GetAllPowerSettings()
  .GetITSMode(PowerSettingsRequest)
  .SetITSMode(PowerSettingsRequest)
  .SetITSAutoTransition(PowerSettingsRequest)
  .GetPowerPlan() / .SetPowerPlan(...)
  .GetUSBChargingMode() / .SetUSBChargingMode(...)
  .GetDynamicGpuMode(...) / .SetDynamicGpuMode(...)
  .ItsAutoTransRestoreDefault() / .GeekModeRestoreDefault()
```

### 2.1 契约类型

`Lenovo.Modern.Contracts.Power`：

```csharp
enum ItsModeType {
    None = 0,
    ItsAuto = 1,          // 智能模式
    MmcCool = 2,          // 静音省电
    MmcPerformance = 3,   // 性能
    MmcGeek = 4,          // 极客
    MmcCustomGeek = 5,    // 自定义极客
}

enum SupportedMmcModeType {  // 位标志，不是 ItsModeType
    None = 0, MmcAuto = 1, MmcCool = 2,
    MmcPerformance = 8, MmcGeek = 16, MmcCustomGeek = 32,
}

class PowerSettingsRequest {
    ItsModeType? ItsMode;
    bool? UISupportGeekMode;
    QuietPerformanceModeType? QuietPerformanceMode;  // None/On/Off
    bool? IsAutoTransitionEnabled;
    int? ResponseDuration;
    Setting[] XmlSettingListDoNotUse;
}

class PowerSettingsResponse {
    ItsModeType? ItsMode;
    SupportedMmcModeType? SupportedMmcMode;
    AcAdapterType? AcAdapterType;
    AcAdapterMode? AcAdapterMode;
    int? AcAdapterWattageInWatts;
    bool? IsDispatcher;
    string WorkingDriver;
    int? ItsVersion;
    string DispatcherVersion;
    bool? IsAutoTransitionEnabled;
    int? ErrorCode;
    bool? ShowBsmAsQuietBsm;
    bool? ShowGeekAsCreator;
    QuietPerformanceModeType? QuietPerformanceMode;
    Setting[] XmlSettingListDoNotUse;
}
```

## 3. 可直接用的 PowerShell 脚本

> 读取/写入都走这里。脚本本身不需要管理员。
> 唯一要求：Vantage 已安装（`%ProgramData%\Lenovo\Vantage\Addins\IdeaNotebookAddin` 存在）。

```powershell
$ErrorActionPreference = 'Stop'

# 加载依赖 + 拿到 PowerAgent 实例；返回 @{ Type; Agent }
function Load-VantagePower {
    $root = Join-Path $env:ProgramData 'Lenovo\Vantage\Addins\IdeaNotebookAddin'
    $dir = Get-ChildItem -LiteralPath $root -Directory -ErrorAction Stop |
        Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } -Descending |
        Select-Object -First 1
    if (-not $dir) { throw 'IdeaNotebookAddin not installed' }

    foreach ($name in 'Newtonsoft.Json.dll',
                      'Lenovo.Vantage.RpcCommon.dll',
                      'Lenovo.Vantage.RpcClient.dll',
                      'Lenovo.VantageService.Utilities.dll',
                      'Lenovo.VantageService.UtilitiesExtensions.dll',
                      'PowerContract.dll') {
        $path = Join-Path $dir.FullName $name
        if (Test-Path -LiteralPath $path) {
            try { [Reflection.Assembly]::LoadFrom($path) | Out-Null } catch {}
        }
    }

    $assembly = [Reflection.Assembly]::LoadFrom((Join-Path $dir.FullName 'IdeaPowerAgent.dll'))
    $type = $assembly.GetType('IdeaNotebookAddin.PowerAgent')
    $agent = $type.GetMethod('GetInstance', [Reflection.BindingFlags]'Public,Static,Instance').Invoke($null, @())
    return @{ Type = $type; Agent = $agent }
}

# 读当前模式（返回 MmcCool / ItsAuto / MmcPerformance / ...）
function Get-VantageItsMode($ctx) {
    $request = [Activator]::CreateInstance([Lenovo.Modern.Contracts.Power.PowerSettingsRequest])
    $response = $ctx.Type.GetMethod('GetITSMode').Invoke($ctx.Agent, @($request))
    return $response.ItsMode
}

# 设置模式（'ItsAuto' / 'MmcCool' / 'MmcPerformance' / ...）
function Set-VantageItsMode($ctx, [string]$mode) {
    $request = [Activator]::CreateInstance([Lenovo.Modern.Contracts.Power.PowerSettingsRequest])
    $request.ItsMode = [Lenovo.Modern.Contracts.Power.ItsModeType]$mode
    $response = $ctx.Type.GetMethod('SetITSMode').Invoke($ctx.Agent, @($request))
    return $response.ItsMode
}

# 用法
$ctx = Load-VantagePower
Get-VantageItsMode $ctx                 # 例如 MmcCool
Set-VantageItsMode $ctx 'ItsAuto'      # 智能
Set-VantageItsMode $ctx 'MmcCool'      # 静音省电
```

### 3.1 验证 / 恢复

实测流程（普通用户）：

```powershell
$ctx = Load-VantagePower
"before=$(Get-VantageItsMode $ctx)"
Set-VantageItsMode $ctx 'ItsAuto'
"after_auto=$(Get-VantageItsMode $ctx)"
Set-VantageItsMode $ctx 'MmcCool'
"after_cool=$(Get-VantageItsMode $ctx)"
```

输出应类似：

```
before=MmcCool
after_auto=ItsAuto
after_cool=MmcCool
```

## 4. 集成到 tupa-launcher（Rust）

### 4.1 作为 `:switch` 的 Toggle

`actions.rs` 的内置条目（或用户 `actions.toml`）：

```toml
[[items]]
id = "vantage-mode"
kind = "toggle"
title = "静音省电"
subtitle = "Lenovo Vantage"
group = "电源"
get = 'powershell -NoProfile -EncodedCommand <base64>'
on  = 'powershell -NoProfile -EncodedCommand <base64>'
off = 'powershell -NoProfile -EncodedCommand <base64>'
on_value = "MmcCool"
```

- `get`：`Load-VantagePower` + `Get-VantageItsMode`，输出模式字符串
- `on`：`Set-VantageItsMode $ctx 'MmcCool'`
- `off`：`Set-VantageItsMode $ctx 'ItsAuto'`
- `on_value = "MmcCool"`

Rust 侧用 `actions.rs` 的 `powershell(script)`（UTF-16LE base64 + `-EncodedCommand`）包装，
不要自己拼引号。注意：tupa 的 `run_script` 对 `-EncodedCommand` 会直接 spawn
`powershell.exe`，绕开 `cmd /C` 的 8191 字符限制。

### 4.2 如果要和 Windows Energy Saver 组合

- Vantage `MmcCool` 是 EC/固件层的风扇/功耗策略
- Windows Energy Saver 是系统级节能（动画/透明/后台）
- 两者可以叠加，但 Energy Saver 没有公开的即时 API（注册表要重启；WNF 写入被 ACL 拒绝），
  在 tupa 里是用「Win+A → 方向键 → Space」点快速设置实现的（见 `src/actions.rs` 的 `energy-saver`）

## 5. 坑与注意事项

| 坑 | 正确做法 |
|---|---|
| `New-Object ...PowerSettingsRequest` 传给反射 `Invoke` | 报「无法将 PSObject 转换为 BatteryMgmtRequest/PowerSettingsRequest」；必须用 `[Activator]::CreateInstance(...)` |
| 智能模式写成 `MmcAuto` | `ItsModeType` 里没有 `MmcAuto`，是 `ItsAuto`；`MmcAuto` 是 `SupportedMmcModeType` 的位标志 |
| `GetAllPowerSettings()` 拿不到模式 | 实测返回全空；用 `GetITSMode()` |
| Vantage 升级后路径变化 | 目录名是版本号，必须 `Get-ChildItem` 排序取最高版本 |
| 依赖 DLL 加载失败 | `PowerContract.dll` 必须加载；其它加载失败可忽略（未用到的分支） |
| 权限 | 普通用户即可；不要提权，不要计划任务 |
| Vantage 未安装 | `Load-VantagePower` 抛错；条目在 `:switch` 里显示不可用即可 |
| 反射绑定 | `GetInstance` 用 `[Reflection.BindingFlags]'Public,Static,Instance'` |
| 枚举转换 | `[Lenovo.Modern.Contracts.Power.ItsModeType]$mode` 可直接从字符串转 |
| 读写频率 | `SetITSMode` 是即时生效的；不要每次 `get` 都重新加载 DLL（tupa 的 worker 每次脚本是新进程，可接受） |

## 6. 实测记录（供参考）

- 机器：ThinkBook 14 G8+ 21VG，Windows 11 25H2（build 26200）
- Vantage Addin：`C:\ProgramData\Lenovo\Vantage\Addins\IdeaNotebookAddin\1.0.13.114`
- `GetAllPowerSettings`：所有字段为空
- `GetITSMode`：
  - `ItsMode = MmcCool`
  - `SupportedMmcMode = MmcAuto, MmcCool, MmcPerformance`
  - `QuietPerformanceMode = None`
- `SetITSMode(ItsAuto)` → 读回 `ItsAuto`
- `SetITSMode(MmcCool)` → 读回 `MmcCool`
- 全程普通用户，无 UAC
- 也可以使用 Vantage 的 `QuietPerformanceMode` / `ShowBsmAsQuietBsm`，但 Smart Power Pro 主开关就是 `ItsMode`

## 7. 相关文件

- `src/actions.rs`：`powershell()` / `run_script()`（`-EncodedCommand` 直接 spawn）
- `src/providers/switch.rs`：`:switch` 列表/状态列
- `docs/04-switch-engine.md`：`:switch` 的通用设计
- 电池养护（`IdeaBatteryAgent.dll`）的同类实现，可作为反射模板
