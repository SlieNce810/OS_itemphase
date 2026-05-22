/// 基于线程的爬虫程序
/// 每个学校URL由一个独立线程负责抓取，
/// 多个线程共享同一进程地址空间，通过channel收集结果。
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[path = "../schools.rs"]
mod schools;
use schools::{SchoolInfo, SCHOOLS};

/// 获取当前进程的内存使用量 (RSS, KB)
fn current_rss_kb() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|line| {
                if line.starts_with("VmRSS:") {
                    line.split_whitespace().nth(1).and_then(|v| v.parse().ok())
                } else {
                    None
                }
            })
        })
        .unwrap_or(0)
}

/// 将HTML转换为纯文本（去除HTML标签、脚本、样式）
fn html_to_text(html: &str) -> String {
    let re = regex::RegexBuilder::new(r"<(script|style|noscript)[^>]*?>[\s\S]*?</\1>")
        .case_insensitive(true)
        .build()
        .unwrap();
    let no_script = re.replace_all(html, "");

    let tag_re = regex::Regex::new(r"<[^>]*>").unwrap();
    let no_tags = tag_re.replace_all(&no_script, "");

    let entity_re = regex::Regex::new(r"&[a-zA-Z]+;|&#\d+;|&#x[0-9a-fA-F]+;").unwrap();
    let text = entity_re.replace_all(&no_tags, " ");

    let ws_re = regex::Regex::new(r"[ \t]+").unwrap();
    let compact = ws_re.replace_all(&text, " ");
    let nl_re = regex::Regex::new(r"\n{3,}").unwrap();
    let result = nl_re.replace_all(&compact, "\n\n");

    result.trim().to_string()
}

/// 单个线程的爬取任务
fn crawl_one_task(
    school: &'static SchoolInfo,
    output_dir: PathBuf,
) -> (String, f64, bool, usize) {
    let start = Instant::now();

    match ureq::get(school.url)
        .timeout(Duration::from_secs(30))
        .set("User-Agent", "Mozilla/5.0 (compatible; Crawler/1.0)")
        .call()
    {
        Ok(resp) => {
            let html = resp.into_string().unwrap_or_default();
            let text = html_to_text(&html);
            let content_len = text.len();

            let filepath = output_dir.join(format!("{}.txt", school.name));
            if let Err(e) = fs::write(&filepath, &text) {
                eprintln!("  写入文件失败 {}: {}", school.name, e);
                (school.name.to_string(), start.elapsed().as_secs_f64() * 1000.0, false, 0)
            } else {
                (school.name.to_string(), start.elapsed().as_secs_f64() * 1000.0, true, content_len)
            }
        }
        Err(e) => {
            eprintln!("  请求失败 {}: {}", school.name, e);
            (school.name.to_string(), start.elapsed().as_secs_f64() * 1000.0, false, 0)
        }
    }
}

fn main() {
    let start_total = Instant::now();
    let mem_before = current_rss_kb();

    // 确定输出目录
    let output_dir = {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        dir.pop();
        dir.push("Docs");
        dir
    };
    fs::create_dir_all(&output_dir).expect("无法创建输出目录");

    println!("========================================");
    println!("  基于线程的爬虫 (Thread-based Crawler)");
    println!("========================================");
    println!("学校总数: {}", SCHOOLS.len());
    println!("输出目录: {}", output_dir.display());
    println!();

    // 创建channel收集结果
    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::new();

    // 为每个学校创建一个线程
    for school in SCHOOLS {
        let tx = tx.clone();
        let dir = output_dir.clone();

        let handle = thread::spawn(move || {
            let result = crawl_one_task(school, dir);
            tx.send(result).ok();
        });

        handles.push(handle);
    }

    // 等待所有线程完成
    println!("已启动 {} 个线程, 等待完成...", handles.len());
    for handle in handles {
        handle.join().ok();
    }
    drop(tx); // 关闭发送端

    let total_time = start_total.elapsed();
    let mem_after = current_rss_kb();

    // 收集结果
    let mut total_success = 0usize;
    let mut total_fail = 0usize;
    let mut latencies: Vec<f64> = Vec::new();

    while let Ok((name, latency, success, content_len)) = rx.try_recv() {
        if success {
            total_success += 1;
            println!("✓ {} : 耗时 {:.0}ms, 文本长度 {} 字节", name, latency, content_len);
        } else {
            total_fail += 1;
            println!("✗ {} : 耗时 {:.0}ms", name, latency);
        }
        latencies.push(latency);
    }

    // 统计信息
    let mut sorted = latencies.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    println!();
    println!("========================================");
    println!("           性能统计");
    println!("========================================");
    println!("总请求数:       {}", SCHOOLS.len());
    println!("成功请求数:     {}", total_success);
    println!("失败请求数:     {}", total_fail);
    println!("总耗时:         {:.2} ms", total_time.as_secs_f64() * 1000.0);
    println!("吞吐率:         {:.2} 请求/秒",
             if total_time.as_secs_f64() > 0.0 {
                 SCHOOLS.len() as f64 / total_time.as_secs_f64()
             } else {
                 0.0
             });

    if !sorted.is_empty() {
        let sum: f64 = sorted.iter().sum();
        let avg = sum / sorted.len() as f64;
        let min = sorted.first().unwrap();
        let max = sorted.last().unwrap();
        let med = sorted[sorted.len() / 2];
        let p95_idx = ((sorted.len() as f64) * 0.95).ceil() as usize - 1;
        let p95 = sorted[p95_idx.min(sorted.len() - 1)];

        println!();
        println!("延迟分布 (ms):");
        println!("  最小值:  {:.2}", min);
        println!("  平均值:  {:.2}", avg);
        println!("  中位数:  {:.2}", med);
        println!("  P95:     {:.2}", p95);
        println!("  最大值:  {:.2}", max);
    }

    println!();
    println!("内存开销 (RSS):");
    println!("  爬取前:  {} KB ({:.1} MB)", mem_before, mem_before as f64 / 1024.0);
    println!("  爬取后:  {} KB ({:.1} MB)", mem_after, mem_after as f64 / 1024.0);
    println!("  增量:    {} KB ({:.1} MB)",
             mem_after.saturating_sub(mem_before),
             mem_after.saturating_sub(mem_before) as f64 / 1024.0);

    println!();
    println!("所有纯文本文件已保存至: {}", output_dir.display());
}
