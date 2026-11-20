//! 诊断探针：逐条跑应用的图标「解析 + 提取」，打印耗时与结果。
//!
//! ```bash
//! cargo run --release --features gui --example icon_thread_probe              # 串行（旧版 worker 的做法）
//! cargo run --release --features gui --example icon_thread_probe -- --parallel # 3 条线程共享队列（现在）
//! ```
//!
//! 输出 `-` = 拿不到图标（`appicon::load` 返回 `None`），`+` = 拿到了。
//! 排查「面板上某些行只有 chip」时先用它区分**提取失败**和**还没轮到**。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tupa_launcher::gui::appicon;

/// 首批行数：`prefetch_icons` 发的是 `first-2 .. first+visible+2`（首屏实测 13 行）
fn first_batch(total: usize) -> usize {
    total.min(13)
}

fn probe(command: &str, title: &str) -> (bool, u128, String) {
    let started = Instant::now();
    let (ok, detail) = if let Some(item) = appicon::shell_item_for(command) {
        (
            appicon::load(std::path::Path::new(&item)).is_some(),
            format!("uwp {item}"),
        )
    } else if let Some(exe) = appicon::exe_for(command, title) {
        if exe.is_file() || exe.to_string_lossy().starts_with("shell:") {
            (appicon::load(&exe).is_some(), exe.display().to_string())
        } else {
            (false, format!("非文件 {}", exe.display()))
        }
    } else {
        (false, "exe_for → None".to_string())
    };
    (ok, started.elapsed().as_millis(), detail)
}

fn main() {
    let parallel = std::env::args().any(|flag| flag == "--parallel");
    let cfg = tupa_launcher::config::Config::load().expect("读不到配置");
    let apps: Arc<Vec<(String, String)>> = Arc::new(
        cfg.apps
            .iter()
            .map(|app| (app.command.clone(), app.name.clone()))
            .collect(),
    );

    let started = Instant::now();
    let batch_at = if parallel {
        run_parallel(apps.clone())
    } else {
        run_serial(apps.clone())
    };

    println!(
        "\n{}：{} 条 / 总计 {}ms / 首批 {} 条 {}ms",
        if parallel { "并行(3)" } else { "串行" },
        apps.len(),
        started.elapsed().as_millis(),
        first_batch(apps.len()),
        batch_at
    );
}

fn run_serial(apps: Arc<Vec<(String, String)>>) -> u128 {
    let batch = first_batch(apps.len());
    let started = Instant::now();
    std::thread::spawn(move || {
        appicon::ensure_thread_com();
        let mut at = 0u128;
        for (index, (command, title)) in apps.iter().enumerate() {
            let (ok, ms, detail) = probe(command, title);
            println!("{} {ms:>5}ms  {title:<28} {detail}", if ok { '+' } else { '-' });
            if index + 1 == batch {
                at = started.elapsed().as_millis();
            }
        }
        at
    })
    .join()
    .expect("worker panic")
}

fn run_parallel(apps: Arc<Vec<(String, String)>>) -> u128 {
    let (task_tx, task_rx) = std::sync::mpsc::channel::<(String, String)>();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let task_rx = Arc::new(Mutex::new(task_rx));
    let started = Instant::now();

    for _ in 0..3 {
        let rx = task_rx.clone();
        let tx = done_tx.clone();
        std::thread::spawn(move || {
            appicon::ensure_thread_com();
            loop {
                // 锁只包 `recv`（和 `Panel::new` 里的 worker 池一样）
                let task = {
                    let queue = rx.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    queue.recv()
                };
                let Ok((command, title)) = task else {
                    break;
                };
                let (ok, ms, detail) = probe(&command, &title);
                let _ = tx.send((ok, ms, title, detail));
            }
        });
    }
    drop(done_tx);

    for (command, title) in apps.iter() {
        let _ = task_tx.send((command.clone(), title.clone()));
    }
    drop(task_tx);

    let batch = first_batch(apps.len());
    let mut batch_at: Option<u128> = None;
    let mut seen = 0usize;
    while let Ok((ok, ms, title, detail)) = done_rx.recv() {
        println!("{} {ms:>5}ms  {title:<28} {detail}", if ok { '+' } else { '-' });
        seen += 1;
        if seen == batch && batch_at.is_none() {
            batch_at = Some(started.elapsed().as_millis());
        }
    }
    println!("（见到 {seen} 条）");
    batch_at.unwrap_or_else(|| started.elapsed().as_millis())
}

#[allow(dead_code)]
fn unused(_: Duration) {}
