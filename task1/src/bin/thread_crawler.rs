// ========================================================================
// 基于线程的爬虫程序
// ========================================================================
// 核心思路：每爬一个学校，就创建一个操作系统线程去下载网页。
// 多个线程共享同一个进程的地址空间，通过 channel 汇总结果。
// 对比进程：线程更轻量、创建更快，但共享内存需考虑线程安全。
// ========================================================================
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crawler::common::{current_rss_kb, html_to_text, print_latency_stats, print_memory_stats};
use crawler::schools::{SchoolInfo, SCHOOLS};

/// 单个线程的爬取任务
///
/// 在独立线程中运行，通过 ureq 库发起HTTP请求
/// 返回值：(学校名称, 耗时ms, 是否成功, 文本长度)
fn crawl_one_task(
    school: &'static SchoolInfo,
    output_dir: PathBuf,
) -> (String, f64, bool, usize) {
    let start = Instant::now();

    match ureq::get(school.url)
        .timeout(Duration::from_secs(30))   // 30秒超时，防止卡死
        .set("User-Agent", "Mozilla/5.0 (compatible; Crawler/1.0)")
        .call()
    {
        Ok(response) => {
            let html = response.into_string().unwrap_or_default();
            let text = html_to_text(&html);
            let content_len = text.len();

            let filepath = output_dir.join(format!("{}.txt", school.name));
            if let Err(error) = fs::write(&filepath, &text) {
                eprintln!("  写入文件失败 {}: {}", school.name, error);
                (school.name.to_string(), start.elapsed().as_secs_f64() * 1000.0, false, 0)
            } else {
                (school.name.to_string(), start.elapsed().as_secs_f64() * 1000.0, true, content_len)
            }
        }
        Err(error) => {
            eprintln!("  请求失败 {}: {}", school.name, error);
            (school.name.to_string(), start.elapsed().as_secs_f64() * 1000.0, false, 0)
        }
    }
}

fn main() {
    let start_total = Instant::now();
    let mem_before = current_rss_kb();

    // --- 准备输出目录 ---
    let output_dir = {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        dir.pop();
        dir.push("Docs");
        dir.push("高校名称和官方网站");
        dir
    };
    fs::create_dir_all(&output_dir).expect("无法创建输出目录");

    println!("========================================");
    println!("  基于线程的爬虫 (Thread-based Crawler)");
    println!("========================================");
    println!("学校总数: {}", SCHOOLS.len());
    println!("输出目录: {}", output_dir.display());
    println!();

    // --- 创建管道，用于线程间传递结果 ---
    // sender：各线程往里塞结果；receiver：主线程从中取结果
    let (sender, receiver) = mpsc::channel();
    let mut handles = Vec::new();

    // --- 为每个学校创建一个线程 ---
    for school in SCHOOLS {
        let sender = sender.clone();  // 每个线程拿一份副本
        let dir = output_dir.clone();

        let handle = thread::spawn(move || {
            let result = crawl_one_task(school, dir);
            sender.send(result).ok();  // 忽略发送失败（主线程可能已退出）
        });

        handles.push(handle);
    }

    // --- 等待所有线程完成 ---
    println!("已启动 {} 个线程, 等待完成...", handles.len());
    for handle in handles {
        handle.join().ok();  // 忽略线程崩溃
    }
    drop(sender); // 关闭发送端，让 receiver 知道没有更多数据

    let total_time = start_total.elapsed();
    let mem_after = current_rss_kb();

    // --- 收集所有线程的结果 ---
    let mut total_success = 0usize;
    let mut total_fail = 0usize;
    let mut latencies: Vec<f64> = Vec::new();

    while let Ok((name, latency, success, content_len)) = receiver.try_recv() {
        if success {
            total_success += 1;
            println!("✓ {} : 耗时 {:.0}ms, 文本长度 {} 字节", name, latency, content_len);
        } else {
            total_fail += 1;
            println!("✗ {} : 耗时 {:.0}ms", name, latency);
        }
        latencies.push(latency);
    }

    // --- 打印统计结果 ---
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

    print_latency_stats(&latencies);
    print_memory_stats(mem_before, mem_after);

    println!();
    println!("所有纯文本文件已保存至: {}", output_dir.display());
}
