# 应用内开关 / 选项 / 数值（设计文档）

> 目标：把启动器从「执行一条命令就关掉」扩展成「在面板里看状态、切开关、调数值」。
> 例：在面板里直接开关联想 Vantage 的「电源养护模式」，不用点开 Vantage。
>
> 状态：**已实现（v0.7）**。第 1 节是通用设计，第 2 节是本机（联想）的实测结果。
> 实现备注见文末，和第 1 节有出入的地方以「实现备注」为准。
>
> **改名注记（2026-10）**：命令已从 `:switch` 改成 `:set`。本文是设计文档，正文按当时的名字
> `switch` 书写，不改历史；当前命令与现状见 [`../STATE.md`](../STATE.md)。

## 1. 通用设计

### 1.1 条目类型

`Item` 现在只有「动作」，开关类需要多一个**交互类型**和**当前状态**：

```rust
pub enum Control {
    /// 回车型：执行后隐藏面板（现有行为）
    Run,
    /// 空格切换开 / 关
    Toggle { value: bool },
    /// ←→ 在选项里切换
    Choice { options: Vec<String>, index: usize },
    /// ←→ 按 step 调整，回车或空格提交
    Number { value: i64, min: i64, max: i64, step: i64, suffix: String },
}
```

`Item` 增加 `control: Control`（默认 `Run`，所有现有 provider 不用改）。

### 1.2 交互

| 键 | Run | Toggle | Choice / Number |
|---|---|---|---|
| `空格` | 插入空格 | 不绑定（见实现备注） | 不绑定 |
| `←` `→` | 移动光标 | 本地翻转 | 上一项 / 下一项、减 / 加 step |
| `Enter` | 执行 + 隐藏 | 提交（**不隐藏**） | 提交（不隐藏） |
| 执行后 | 隐藏面板 | 后台写入完成后重查当前 provider | 同 Toggle |

- 状态显示在最后一列（右对齐）：`开` / `关` / `60%` / `均衡`
- `←→` 只改本地显示值，`Enter` 才真正写出去
- 执行走后台 worker（脚本可能几百毫秒），完成后发 `Evt::Applied`，App 重查当前 provider
  **并保留原来的选中行**

`Action` 新增：

```rust
pub enum Action {
    // …
    /// 把开关设成某个值（由 actions.toml 里的脚本执行）
    SetControl { id: String, value: String },
}
```

### 1.3 配置驱动：`~/.config/tupa-launcher/actions.toml`

不写 Rust 就能加开关：

```toml
# 开关
[[items]]
title = "电池养护模式"
subtitle = "Lenovo Vantage"
kind = "toggle"
group = "电源"
# 读取当前值（stdout 第一行，去掉首尾空白）
get = 'powershell -NoProfile -File C:\Users\me\.config\tupa-launcher\scripts\battery-get.ps1'
on = 'powershell -NoProfile -File ...\battery-set.ps1 -Value 1'
off = 'powershell -NoProfile -File ...\battery-set.ps1 -Value 0'
# get 输出这个字符串算「开」，不写则支持 1/true/on/开/enabled
on_value = "1"

# 选项
[[items]]
title = "性能模式"
kind = "choice"
options = ["安静", "均衡", "性能"]
get = '...\perf-get.ps1'
set = '...\perf-set.ps1 -Mode "{value}"'

# 数值
[[items]]
title = "屏幕亮度"
kind = "number"
min = 0
max = 100
step = 10
suffix = "%"
get = '...\brightness-get.ps1'
set = '...\brightness-set.ps1 -Value {value}'
```

- `{value}` 会替换成实际值（选项是文本，数值是数字）
- 所有命令都走 `cmd /C` 在 worker 线程里跑（可阻塞），**带超时**（比如 5s），失败就把 stderr 第一行显示在状态栏
- `group` 用于 `:set 电源` 过滤，也决定显示顺序

### 1.4 新增 provider

```
:set            列出所有开关 / 选项 / 数值
:set 电源       按 group 或标题过滤
:set:all        包含当前设备不支持的项（见 get 失败标记）
```

- 每个条目 `get` 一次拿当前值（并发地跑，或串行但限时）
- `get` 失败（脚本报错 / 设备不支持）→ 该条目灰掉，状态列显示「不可用」，回车给出原因

### 1.5 需要改的地方

| 文件 | 改动 |
|---|---|
| `providers/mod.rs` | `Item.control`、`Action::SetControl`、`Evt::Refresh` |
| `app.rs` | 空格 / ←→ 分支；执行不隐藏的路径；执行后刷新当前 provider |
| `ui.rs` | 状态列（最后一列右对齐）、不可用条目变灰 |
| `actions.rs`（新） | actions.toml 解析、脚本执行、超时 |
| `providers/switch.rs`（新） | provider 实现 |
| `exec.rs` | `SetControl` 分支（跑脚本） |

## 2. 本机实测：ThinkBook 14 G8+（21VG）

结论：**WMI / BIOS 都改不了养护模式，但 Vantage 的托管程序集可以（且不需要管理员）**。

| 接口 | 结果 |
|---|---|
| `LENOVO_GAMEZONE_DATA` 实例（普通用户） | 拒绝访问 `0x80041003` |
| `LENOVO_GAMEZONE_DATA` 的 `Get*` / `Set*` 方法（管理员） | **全部「无效的对象」**（ThinkBook 不是 Gaming 系列，方法未实现） |
| `Lenovo_BiosSetting` | 没有 `BatteryChargeMode` / 养护相关项 |
| `Lenovo_GetBiosSelections("BatteryChargeMode")` | `ret=True` 但 `Selections` 为空（不支持） |
| `LENOVO_UTILITY_DATA.GetIfSupportOrVersion(0..64)` | 只返回版本号，没有开关语义 |
| `LENOVO_OTHER_METHOD.GetFeatureValue` | 只有部分 ID 可用，没有 `0x03010001`（充电限制） |
| `WmiMonitorBrightness` / `WmiMonitorBrightnessMethods` | **可读可写**（`WmiSetBrightness`，无需提权） |
| `HKCU\...\Themes\Personalize` | **可读可写**（深色模式，无需提权） |
| `HKLM\...\Control\Power\EnergySaverState` | 可写（需管理员），但 **重启后才生效**；WinRT 的 `EnergySaverStatus` 在 24H2 手动开启时也会报 `Off` |
| 内置「节能」电源计划 `a1841308-...` | **可即时切换**（`powercfg /setactive`，普通用户） |

### 电池养护：反射 Vantage 的 Addin

本机装了 Vantage（`LenovoVantageService` + `IdeaNotebookAddin` 进程在跑），它的
`C:\ProgramData\Lenovo\Vantage\Addins\IdeaNotebookAddin\<版本>\IdeaBatteryAgent.dll`
暴露了托管接口：

```csharp
IdeaNotebookAddin.BatteryAgent.GetInstance()
  .GetBatteryChargeMode()                  // -> Storage / Normal / Quick
  .SetBatteryChargeMode(BatteryMgmtRequest)
```

用 PowerShell 反射加载 + 调用即可：**不需要管理员、不需要计划任务、不需要装驱动**。
枚举 `BatteryChargeModeType`：`Normal = 0`、`Storage = 1`、`Quick = 2`。

所以计划任务提权方案**没有采用**：它只能给权限，给不了 WMI 里不存在的功能，
而且会往系统里写计划任务。

### 节能模式（Energy Saver）：最终用快速设置点击

调研过两条路，都不行：

1. **WNF**：`WNF_PO_ENERGY_SAVER_STATE`（`0x41C6013D_A3BC2075`）能读，但写入：
   - 普通用户 / 管理员 → `0xC0000022`（ACCESS_DENIED）
   - SYSTEM 计划任务 → 写入成功，但读回为空：状态是 **Session scope**，
     SYSTEM 在 Session 0，与用户会话（Session 1）隔离，影响不到用户会话
2. **UIAutomation**：快速设置是 WinUI3，Windows PowerShell 5.1 的 UIA2 用
   `FromHandle` 拿到窗口后子元素为 0；pwsh 7 的 UIA3 COM `FindAll` 会挂死

最终落地：**键盘优先、鼠标回退**。

- `Win+A` 打开快速设置（轮询 `ControlCenterWindow`，窗口一出现就继续，不等动画）
- `Home` 回第一个按钮 → `Right` ×2 → `Down` → `Space` 切到「节能模式」
  （焦点在 3×N 网格里按方向键移动，比 Tab 顺序稳定；实测 1↔2 可重复）
- 读 `EnergySaverState` 校验；没变就用鼠标点击回退（坐标按 DPI 算，当前 `(2894, 1298)`）
- `Win+A` 关闭快速设置

实测一次切换约 **1 秒**（鼠标版 4.4 秒）。注意方向键序列依赖快速设置的按钮布局：
如果调整了固定的按钮，需要改 `Right x2 + Down` 的步数。

> 内置的 `-EncodedCommand` 脚本现在直接 spawn `powershell.exe`，不再经过 `cmd /C`，
> 否则长脚本的 base64 会撞上 8191 字符的命令行限制。

### 原设计里的提权方案（留档，未采用）

1. **每次弹 UAC**：最简单，但每点一次开关弹一次，体验差
2. **计划任务按需提权**：首次一次 UAC 建任务，之后 `schtasks /run`，不弹窗、
   不常驻（不占开机时间）；但脚本的 stdout 拿不到，需要写临时文件回传
3. 自建 Windows 服务：安装成本高，不推荐

> 如果以后要做真正需要管理员的功能（BIOS 写入 / 驱动 IOCTL），再考虑第 2 种。

## 3. 实现顺序建议

1. ✅ `Item.control` + `[:set]` 的只读版本
2. ✅ 第一个真正可写的开关：免提权的**深色模式** + **屏幕亮度**
3. ✅ `actions.toml` 驱动 + `Action::SetControl`
4. ✅ 电池养护模式：改为**反射 Vantage Addin**（免提权，原计划的计划任务没用到）

## 4. 实现备注（v0.7）

- **空格不绑定控件**：空格是搜索词 / 参数的分隔符（`web rust async`、`:set 电源`），
  绑到切换上会让「在 `:set` 后面打过滤词」直接触发一次写操作。所以控件用 `Enter` 应用、
  `←/→` 调整，状态栏右下角会切换成对应提示。
- `Evt::Applied` 不再用 `Evt::Refresh`：worker 执行完写操作后把结果（成功文案 / 错误）
  一起带回来；`Apply` 请求**不会被 worker 丢弃**（查询才会）。
- 脚本执行：`cmd /C` + 5 秒超时 + stdout/stderr 两个读线程（防止管道写满卡死）；
  写命令额外设置环境变量 `TUPA_VALUE`，内置脚本靠它取参数。
- 内置 PowerShell 用 `-EncodedCommand`（UTF-16LE + base64，自己编码），
  绕开 `cmd /C` 对引号 / 管道符的转义。
- 内置项：深色模式（Toggle）、屏幕亮度（Number 0~100 step 10）、
  电池养护模式（Toggle，`Storage` ↔ `Normal`）、节能模式（Toggle，快速设置方向键）、
  保持唤醒（Toggle，nosleep 进程）。用户 `actions.toml` 里同 `id` 的条目覆盖内置项。
- 电池养护脚本会自己找 `C:\ProgramData\Lenovo\Vantage\Addins\IdeaNotebookAddin\*`
  里版本最高的目录，所以 Vantage 升级后不用改代码；Vantage 没装时 `get` 失败，条目自动隐藏。
- 读不到状态的条目默认隐藏，`:set:all` 才显示（显示「不可用」）。
