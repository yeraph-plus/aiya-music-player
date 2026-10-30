//! 扫描基准工具（docs/29 批 B 的基线/回归口径）。
//!
//! 目的是给「并行到底快了多少」一个**可复现的数字**，而不是靠体感。
//! 用法：
//!
//! ```text
//! cargo run --release -p aiya-data --example scan_bench -- \
//!     --root "D:\CkMsr\Music" --db /tmp/bench.db --runs 3 --artwork /tmp/bench-art
//! ```
//!
//! 参数：
//!   --root <dir>      扫描根，可重复给多个（并集单库）
//!   --db <path>       SQLite 文件路径。**保留**，这样同一 DB 连跑多次就是
//!                     "首次全量 → 增量重扫"的真实对照（文件不存在则新建）
//!   --runs <n>        跑几轮，默认 3；输出每轮 + 中位数
//!   --artwork <dir>   提取封面并写到该缓存目录（不给 = 不提取，排除封面 IO）
//!   --quiet           不打印逐轮进度回调（避免进度打点本身影响计时）
//!
//! 为什么单独一个 example 而不是 #[test]：它要跑真实曲库、耗时以分钟计，
//! 而且要读真实磁盘（Defender 实时扫描介入）。测试套件里塞这个只会让人把
//! `cargo test` 关掉。基准与门禁分开。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use aiya_data::db::open_conn;
use aiya_data::scanner::{scan_roots_ext, ScanProgress};

struct Args {
    roots: Vec<PathBuf>,
    db: PathBuf,
    runs: usize,
    artwork: Option<PathBuf>,
    quiet: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut roots = Vec::new();
    let mut db: Option<PathBuf> = None;
    let mut runs = 3usize;
    let mut artwork: Option<PathBuf> = None;
    let mut quiet = false;

    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => roots.push(PathBuf::from(it.next().ok_or("--root 缺值")?)),
            "--db" => db = Some(PathBuf::from(it.next().ok_or("--db 缺值")?)),
            "--runs" => {
                runs = it
                    .next()
                    .ok_or("--runs 缺值")?
                    .parse()
                    .map_err(|e| format!("--runs: {e}"))?
            }
            "--artwork" => {
                // 封面缓存目录跟 DB 放一起，方便删库重来
                artwork = Some(PathBuf::from(it.next().ok_or("--artwork 缺值")?));
            }
            "--quiet" => quiet = true,
            other => return Err(format!("未知参数: {other}")),
        }
    }
    if roots.is_empty() {
        return Err("至少要一个 --root".into());
    }
    let db = db.ok_or("必须给 --db（用文件库才能量增量重扫）")?;
    Ok(Args {
        roots,
        db,
        runs,
        artwork,
        quiet,
    })
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("参数错误: {e}");
            std::process::exit(2);
        }
    };

    println!("== 扫描基准 ==");
    println!("根目录: {:?}", args.roots);
    println!("数据库: {}", args.db.display());
    println!("轮次  : {}", args.runs);
    println!(
        "封面  : {}",
        match &args.artwork {
            Some(p) => format!("启用 → {}", p.display()),
            None => "关闭".to_string(),
        }
    );
    println!();

    let mut conn = match open_conn(&args.db) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("打开数据库失败: {e}");
            std::process::exit(1);
        }
    };

    let mut elapsed_samples: Vec<u64> = Vec::with_capacity(args.runs);
    for run in 1..=args.runs {
        let started = Instant::now();
        let mut progress_events = 0usize;
        let mut last_phase = String::new();

        let result = scan_roots_ext(
            &mut conn,
            &args.roots,
            args.artwork.as_deref(),
            || false,
            |p: ScanProgress| {
                progress_events += 1;
                last_phase = p.phase.clone();
                if !args.quiet && p.phase != "walking" {
                    // 只在批次边界打一行，别让 I/O 成为计时噪音源
                    eprintln!(
                        "  [{}] {} scanned={}/{:?} parsed={} imported={} skip={} fail={}",
                        run, p.phase, p.scanned, p.total, p.parsed, p.imported, p.skipped, p.failed
                    );
                }
            },
        );

        let wall = started.elapsed().as_millis() as u64;
        match result {
            Ok(r) => {
                elapsed_samples.push(wall);
                println!(
                    "run {run}: wall={wall}ms  reported={}ms  scanned={} imported={} unchanged={} failed={} walk_err={} events={} last_phase={}",
                    r.elapsed_ms, r.scanned, r.imported, r.unchanged, r.failed, r.walk_errors,
                    progress_events, last_phase
                );
                if !r.errors.is_empty() {
                    println!("  前 5 条错误:");
                    for e in r.errors.iter().take(5) {
                        println!("    {} ← {}", e.path, e.message);
                    }
                }
            }
            Err(e) => {
                eprintln!("run {run} 失败: {e}");
                std::process::exit(1);
            }
        }
    }

    // 中位数：偶数取中间两数均值，避免"最小值最好看"的自欺
    let mut sorted = elapsed_samples.clone();
    sorted.sort_unstable();
    let median = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2
    };
    let mut summary = BTreeMap::new();
    summary.insert("min", sorted.first().copied().unwrap_or(0));
    summary.insert("median", median);
    summary.insert("max", sorted.last().copied().unwrap_or(0));
    println!();
    println!("== 汇总（wall ms）: {summary:?} ==");

    // 曲库规模：让数字可换算成"每千文件多少秒"
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .unwrap_or(-1);
    println!("库内曲目: {total}");
    if total > 0 {
        // 单位是**秒/千曲目**：median 是毫秒，先 /1000 换成秒再折算
        println!(
            "每千曲目: {:.2}s",
            (median as f64 / 1000.0) / (total as f64 / 1000.0)
        );
    }
}
