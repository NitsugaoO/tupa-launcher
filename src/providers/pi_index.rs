//! 从 **pi-sessions 的索引**里取会话名（`session_info` 的权威副本）。
//!
//! pi 的自动标题（`pi-sessions` 扩展的 auto-title，每 4 轮重评一次）不是写在
//! 第一条用户消息里，而是写成 jsonl 里的 `{"type":"session_info","name":…}`
//! 条目 —— 而且**可以出现在文件任何位置**（更新是追加写的）。实测本机 448 个
//! 会话里，最后一条 `session_info` 距文件末尾中位 8KB、p90 648KB、最大 33MB，
//! 靠「读前 N 行」或「倒读尾巴」都拿不全。
//!
//! 好在 pi-sessions 自己维护了一份 SQLite 索引：
//!
//! ```text
//! ~/.pi/agent/pi-sessions/index.sqlite
//!   sessions(session_path, session_name, first_user_prompt, message_count, …)
//! ```
//!
//! 它在每个 `turn_end` 由 hook 增量同步（`metadata.hook_updated_at` 就是上一次
//! 写入时间），pi 启动时还会跟 jsonl 对账 —— 所以**不用自己缓存、也不用处理
//! 失效**：标题改了下一轮查询自动就是新的。
//!
//! 索引目录可被 `~/.pi/agent/settings.json` 的 `sessions.index.dir` 改掉，
//! 所以先读配置再拼文件名。**任何一步失败都退化成空表**（调用方回退 jsonl），
//! 不 panic、不打印 —— pi-sessions 没装 / 正在重建 / schema 升级过都属于正常情况。
//!
//! `rusqlite` 不是为这个功能加的：`providers/web.rs` 早就在读 Edge 的 `History`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

/// 索引里「有名字」的会话 → `会话名`，key 已归一成小写反斜杠路径。
///
/// 拿不到（没装 pi-sessions / 库坏了 / 正在重建）就是空表。
pub(super) fn load_names() -> HashMap<String, String> {
    let Some(home) = dirs::home_dir() else {
        return HashMap::new();
    };
    let Ok(conn) = open_index(&index_file(&home)) else {
        return HashMap::new();
    };
    names_from(&conn).unwrap_or_default()
}

/// 会话路径的比较键：Windows 路径大小写不敏感，索引里与 `read_dir` 出来的
/// 分隔符也可能不同，统一成小写 + 反斜杠。
pub(super) fn key_of(path: &str) -> String {
    path.to_lowercase().replace('/', "\\")
}

/// `~/.pi/agent/pi-sessions/index.sqlite`（`sessions.index.dir` 可覆盖）。
fn index_file(home: &Path) -> PathBuf {
    let agent = home.join(".pi").join("agent");
    let custom = std::fs::read_to_string(agent.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| {
            json.get("sessions")?
                .get("index")?
                .get("dir")?
                .as_str()
                .map(str::to_string)
        });
    let dir = match custom {
        Some(dir) => expand_home(&dir, home),
        None => agent.join("pi-sessions"),
    };
    dir.join("index.sqlite")
}

/// 只认 `~` / `~/x` / `~\x`，其余当绝对路径（pi-sessions 的文档是这么约定的）。
fn expand_home(dir: &str, home: &Path) -> PathBuf {
    let rest = dir
        .strip_prefix("~/")
        .or_else(|| dir.strip_prefix("~\\"))
        .or_else(|| (dir == "~").then_some(""));
    match rest {
        Some(rest) => home.join(rest),
        None => PathBuf::from(dir),
    }
}

fn open_index(file: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        file,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // 索引正在被另一个 pi 写时让它等一会儿，别直接失败。
    conn.busy_timeout(Duration::from_millis(200))?;
    Ok(conn)
}

/// 只取名字非空的行（本机 235 条里 84 条为空，那些走 jsonl 回退）。
fn names_from(conn: &Connection) -> Option<HashMap<String, String>> {
    let mut stmt = conn
        .prepare("SELECT session_path, session_name FROM sessions")
        .ok()?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .ok()?;

    let mut names = HashMap::new();
    for (path, name) in rows.flatten() {
        let name = name.trim();
        if !name.is_empty() {
            names.insert(key_of(&path), name.to_string());
        }
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db(rows: &[(&str, &str)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (session_path TEXT PRIMARY KEY, session_name TEXT NOT NULL);",
        )
        .unwrap();
        for (path, name) in rows {
            conn.execute(
                "INSERT INTO sessions (session_path, session_name) VALUES (?1, ?2)",
                [path, name],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn reads_non_empty_names_with_normalized_keys() {
        let conn = memory_db(&[
            (r"C:\Users\x\.pi\agent\sessions\a\1.jsonl", "标题一"),
            (r"C:\Users\x\.pi\agent\sessions\b\2.jsonl", "   "),
        ]);
        let names = names_from(&conn).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(
            names
                .get(&key_of(r"c:\users\X\.pi\agent\sessions\a\1.jsonl"))
                .map(String::as_str),
            Some("标题一")
        );
    }

    #[test]
    fn missing_table_or_column_is_not_fatal() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(names_from(&conn).is_none());

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE sessions (session_path TEXT PRIMARY KEY);")
            .unwrap();
        assert!(names_from(&conn).is_none());
    }

    #[test]
    fn absent_index_yields_no_names() {
        let dir = std::env::temp_dir().join("tupa-pi-index-does-not-exist");
        assert!(load_names_from(&dir).is_empty());
        assert!(open_index(&index_file(&dir)).is_err());
    }

    #[test]
    fn index_file_follows_settings_override() {
        let dir = std::env::temp_dir().join(format!("tupa-pi-index-cfg-{}", std::process::id()));
        let agent = dir.join(".pi").join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        // 默认位置
        assert_eq!(
            index_file(&dir),
            agent.join("pi-sessions").join("index.sqlite")
        );
        // 配置覆盖（`~/` 展开到传入的 home）
        std::fs::write(
            agent.join("settings.json"),
            r#"{"sessions":{"index":{"dir":"~/.pi/my-index"}}}"#,
        )
        .unwrap();
        assert_eq!(
            index_file(&dir),
            dir.join(".pi").join("my-index").join("index.sqlite")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 测试用：和 [`load_names`] 一样，但 home 可指定（不碰真实配置目录）。
    fn load_names_from(home: &Path) -> HashMap<String, String> {
        let Ok(conn) = open_index(&index_file(home)) else {
            return HashMap::new();
        };
        names_from(&conn).unwrap_or_default()
    }
}
