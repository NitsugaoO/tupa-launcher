//! 命令历史：`~/.config/tupa-launcher/history.json`
//!
//! 只在动作**执行成功**后记录（失败的不记，免得污染历史）。
//! 最新的在最前，重复的移到最前，最多 [`LIMIT`] 条。

use std::path::PathBuf;

use anyhow::{Context, Result};

pub const LIMIT: usize = 200;

pub fn path() -> Option<PathBuf> {
    Some(crate::config::config_dir()?.join("history.json"))
}

pub fn load() -> Vec<String> {
    let Some(path) = path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut items: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    items.truncate(LIMIT);
    items
}

pub fn save(items: &[String]) -> Result<()> {
    let Some(path) = path() else {
        return Ok(()); // 没有配置目录就只在内存里留一份
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("无法创建目录 {}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(items).context("序列化历史失败")?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("写入失败：{}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("写入失败：{}", path.display()))?;
    Ok(())
}

/// 把一行输入记进历史（去重前移 + 截断）
pub fn push(items: &mut Vec<String>, line: &str) {
    items.retain(|entry| entry != line);
    items.insert(0, line.to_string());
    items.truncate(LIMIT);
}
