/// 基于协程的爬虫程序（Async/Await）
/// 使用tokio异步运行时，通过协程并发爬取所有学校URL。
/// 协程运行在少数几个OS线程之上，通过异步I/O实现高并发。
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crawler::schools::{SchoolInfo, SCHOOLS};

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
    // Rust 的 regex crate 不支持反向引用(\1)，改用三个独立正则
    let re_script = regex::RegexBuilder::new(r"<script[^>]*?>[\s\S]*?</script>")
        .case_insensitive(true)
        .build()
        .unwrap();
    let re_style = regex::RegexBuilder::new(r"<style[^>]*?>[\s\S]*?</style>")
        .case_insensitive(true)
        .build()
        .unwrap();
    let re_noscript = regex::RegexBuilder::new(r"<noscript[^>]*?>[\s\S]*?</noscript>")
        .case_insensitive(true)
        .build()
        .unwrap();
    let no_script = re_script.replace_all(html, "");
    let no_script = re_style.replace_all(&no_script, "");
    let no_script = re_noscript.replace_all(&no_script, "");

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

/// 单个协程的爬取任务（异步版本）
async fn crawl_one_async(
    school: &SchoolInfo,
    output_dir: &PathBuf,
) -> (String, f64, bool, usize) {
    let url = school.url.to_string();
    let name = school.name;
    let filepath = output_dir.join(format!("{}.txt", name));

    let start = Instant::now();

    // 使用tokio的spawn_blocking来执行阻塞的HTTP请求
    // 这样不会阻塞异步运行时的事件循环
    let result = tokio::task::spawn_blocking(move || {
        ureq::get(&url)
            .timeout(Duration::from_secs(30))
            .set("User-Agent", "Mozilla/5.0 (compatible; Crawler/1.0)")
            .call()
    })
    .await;

    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    match result {
        Ok(Ok(resp)) => {
            let html = resp.into_string().unwrap_or_default();

            // HTML转纯文本（CPU密集型操作，在spawn_blocking中执行）
            let filepath_clone = filepath.clone();
            let text = tokio::task::spawn_blocking(move || html_to_text(&html))
                .await
                .unwrap_or_default();

            let content_len = text.len();

            // 文件写入
            let filepath2 = filepath_clone.clone();
            let write_result = tokio::task::spawn_blocking(move || {
                fs::write(&filepath2, &text)
            })
            .await;

            match write_result {
                Ok(Ok(_)) => (name.to_string(), elapsed_ms, true, content_len),
                _ => {
                    eprintln!("  写入文件失败: {}", name);
                    (name.to_string(), elapsed_ms, false, 0)
                }
            }
        }
        Ok(Err(e)) => {
            eprintln!("  请求失败 {}: {}", name, e);
            (name.to_string(), elapsed_ms, false, 0)
        }
        Err(_) => {
            eprintln!("  spawn_blocking失败: {}", name);
            (name.to_string(), elapsed_ms, false, 0)
        }
    }
}

/// 异步运行的入口（使用信号量控制并发数）
async fn run_async(
    output_dir: &PathBuf,
    concurrency: usize,
) -> Vec<(String, f64, bool, usize)> {
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut handles = Vec::new();

    for school in SCHOOLS {
        let sem = semaphore.clone();
        let dir = output_dir.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            crawl_one_async(school, &dir).await
        });

        handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }

    results
}

#[tokio::main]
async fn main() {
    let start_total = Instant::now();
    let mem_before = current_rss_kb();

    // 确定输出目录
    let output_dir = {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        dir.pop();
        dir.push("Docs");
        dir.push("高校名称和官方网站");
        dir
    };
    fs::create_dir_all(&output_dir).expect("无法创建输出目录");

    let concurrency = 20; // 最大并发数

    println!("========================================");
    println!("  基于协程的爬虫 (Async/Coro-based Crawler)");
    println!("========================================");
    println!("学校总数: {}", SCHOOLS.len());
    println!("最大并发: {}", concurrency);
    println!("输出目录: {}", output_dir.display());
    println!();

    let results = run_async(&output_dir, concurrency).await;

    let total_time = start_total.elapsed();
    let mem_after = current_rss_kb();

    // 统计
    let mut total_success = 0usize;
    let mut total_fail = 0usize;
    let mut latencies: Vec<f64> = Vec::new();

    for (name, latency, success, content_len) in &results {
        if *success {
            total_success += 1;
            println!("✓ {} : 耗时 {:.0}ms, 文本长度 {} 字节", name, latency, content_len);
        } else {
            total_fail += 1;
            println!("✗ {} : 耗时 {:.0}ms", name, latency);
        }
        latencies.push(*latency);
    }

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
