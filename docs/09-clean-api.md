# pi 会话清理：HTML 界面 + HTTP 契约（`/clean`）

> 2026-11-19 决定：`@pi` 整个从 tupa 里移除（慢且不常用），
> 清理功能**不再由 tupa 实现**，改为 `my-pi-plugins` 的 `/clean` 命令起一个本地服务，
> 用浏览器页面操作。tupa 这边只留**前端页面 + 本契约**，代码零依赖。
>
> 交付物：`web/clean/index.html`（单文件、无构建、原生 JS）。

## 为什么换掉 `@pi:clean`

- `@pi` 每次查询要开 175 个 jsonl、读前 80 行 ≈ **0.68s / 88MB**，清理视图更是双份。
- 清理本质是「看一批会话，挑一些归档 / 删掉」，TUI 面板一次只能看十几行、还得记键位；
  浏览器里能排序、能多选、能一眼看完 200 条。
- 规则本身很稳定（下面三条），搬家成本低。

## 目录布局（规则的地基）

```text
~/.pi/agent/
├── sessions/<编码路径>/<session-id>.jsonl     ← 活跃会话
├── session-archive/<编码路径>/<同名>.jsonl    ← 归档（可恢复）
├── session-keywords.json                      ← { "<完整 jsonl 路径>": { summary, keywords } }
└── session-favorites.json                     ← ["<完整 jsonl 路径>", …]
```

`session-archive/` 与 `sessions/` 的子目录同名，所以归档/恢复都是**在同一棵树里移动**。

## 判定规则（`classify`，纯函数，不读盘）

```text
protect  收藏（在 session-favorites.json 里）
protect  最后修改时间在窗口内（默认 14 天；可选 7 / 30）
archive  否则，keywords 非空（已总结）或消息数 ≥ 8
trash    其余（瘦会话）
```

三条要点，改规则时别破坏：

1. **时间窗口是前置条件**，不是并列条件 —— 窗口内一律不动。
2. **收藏永不参与清理**（只加星、不改判定之外的任何东西）。
3. `keywords` 来自 `session-keywords.json`（pi 的总结插件写的），`messages` 数的是
   文件前 80 行里 `role` 为 user/assistant 的消息数（和 tupa 旧实现一致，别改成全文件计数，
   否则同一个会话的判定会和历史记录对不上）。

## HTTP 契约

服务默认监听 `127.0.0.1:4517`（页面同源时用相对路径；直接双击打开时页面回落到这个地址）。
所有响应 `Content-Type: application/json`。

### `GET /` → `index.html`

单文件页面，直接 `readFile` 返回即可。

### `GET /api/sessions?days=14`

`days` ∈ `{7, 14, 30}`，缺省 14。

```json
{
  "windowDays": 14,
  "sessions": [
    {
      "path": "C:\\Users\\me\\.pi\\agent\\sessions\\-C--Users-me-Projects-x\\01ABC.jsonl",
      "title": "tupa-launcher 开工：读 STATE.md",
      "project": "tupa-launcher",
      "summary": "",                  // session-keywords.json 的 summary，没有就是空串
      "keywords": ["tupa-launcher"],  // 同上
      "messages": 12,
      "sizeBytes": 2481234,
      "mtime": 1763590000,            // 秒
      "ageDays": 12.4,
      "favorite": false,
      "disposition": "archive"        // "protect" | "archive" | "trash"
    }
  ]
}
```

按 `mtime` 降序（新的在前）。`title` 的优先级沿用短版：
`session-keywords.json` 的 summary > jsonl 里最后一条 `session_info.name` > 首条用户消息 > 文件名。
**只读前 80 行**，不要把几 MB 的会话全读进来。

### `GET /api/archive`

同结构，列出 `session-archive/` 下的全部会话（**不受时间窗口限制**，因为它们已经被移出去了）。
`disposition` 固定 `"archive"`，`ageDays` 用 mtime。

### `POST /api/apply`

```json
{ "ops": [ { "path": "…/x.jsonl", "mode": "archive" | "trash" | "restore" } ] }
```

```json
{ "results": [ { "path": "…/x.jsonl", "ok": true, "message": "已归档 xxx" } ] }
```

逐条执行，**一条失败不影响后面**，全部执行完再返回。失败时 `ok: false`，`message` 给人看。

三条硬约束（照抄 tupa 旧实现 `run_session`，别简化）：

| 模式 | 做法 | 约束 |
|---|---|---|
| `archive` | `sessions/<rel>` → `session-archive/<rel>` | 目标是 `rename`；**目标已存在时绝不覆盖**，直接报错 |
| `trash` | 优先 `trash <path>`（进系统回收站） | `trash` 不可用时才真删；两者都失败要报错 |
| `restore` | `session-archive/<rel>` → `sessions/<rel>` | 目标已存在时**加时间戳后缀**（`x-restored-HHmmss.jsonl`），不覆盖 |

额外约束：

- `path` 必须在 `sessions/` 或 `session-archive/` 里（`path.relative` 不能以 `..` 开头），
  拒绝任意路径 —— 这是删文件接口，必须挡目录穿越。
- 目标父目录不存在时先创建（会话目录是 `-C--Users-…` 这种编码路径，归档树里可能还没建）。
- 建议在 `/clean` 里先算清单、**弹一次确认**再调 `apply`；页面已经在 `archive` / `trash`
  两个按钮上做了 `confirm`，但服务端不该假设它一定有。

## 参考实现骨架（Node，供 `/clean` 用）

```ts
// plugins/pi-clean/server.ts
import { createServer } from "node:http";
import { readFile, readdir, stat, rename, mkdir } from "node:fs/promises";
import { join, relative, dirname, basename } from "node:path";

const AGENT = join(homedir(), ".pi", "agent");
const PORT = 4517;

// GET /            → 读 tupa 仓库里的 web/clean/index.html（或拷贝进本插件）
// GET /api/sessions → collect(join(AGENT, "sessions"), days)
// GET /api/archive  → collect(join(AGENT, "session-archive"), Infinity)
// POST /api/apply   → 逐个 runSession()
```

复用注意：`collect` 就是 tupa 旧 `pi.rs::collect` 的短版，`classify` 是 `clean.rs::classify`
的直译。代码很短（约 120 行），不值得为它保留 Rust 实现。

## 页面自身的行为约定

- 勾选状态按 **`path`** 存（不是行下标）—— 重新排序 / 过滤后选择不能串行。
- 「待清理」视图里 `protect` 的行不可勾选（显示为半透明），避免误删收藏和新会话。
- 两个视图（待清理 / 已归档）互斥，切视图清空选择。
- 执行完自动重新拉一次列表（`apply` 之后 `GET /api/sessions`）。
- 删除 / 归档都会先 `confirm`，并且提示条数。
