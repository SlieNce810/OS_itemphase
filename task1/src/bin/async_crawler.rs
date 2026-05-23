// ========================================================================
// 基于协程的爬虫程序（Async/Await）
// ========================================================================
// 核心思路：使用 tokio 异步运行时，通过协程并发爬取所有学校。
// 协程运行在少数几个OS线程之上，遇到I/O等待时自动让出执行权。
// 对比线程：协程更轻量（一个线程可跑上万协程），但代码编写更复杂。
// ========================================================================
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crawler::common::{current_rss_kb, html_to_text, print_latency_stats, print_memory_stats};
use crawler::schools::{SchoolInfo, SCHOOLS};

/// 单个协程的爬取任务（异步版本）
///
/// 返回值：(学校名称, 耗时ms, 是否成功, 文本长度)
///
/// 为什么用 spawn_blocking 而不是 async HTTP 客户端？
/// 因为 ureq 是同步库，直接在 async 函数调用会阻塞整个事件循环。
/// spawn_blocking 把阻塞操作丢到专门的线程池中执行。
async fn crawl_one_async(
    school: &SchoolInfo,
    output_dir: &PathBuf,
) -> (String, f64, bool, usize) {
    let url = school.url.to_string();
    let name = school.name;
    let filepath = output_dir.join(format!("{}.txt", name));

    let start = Instant::now();

    // --- 发起HTTP请求（阻塞操作，交给线程池执行）---
    let result = tokio::task::spawn_blocking(move || {
        ureq::get(&url)
            .timeout(Duration::from_secs(30))
            .set("User-Agent", "Mozilla/5.0 (compatible; Crawler/1.0)")
            .call()
    })
    .await;

    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    match result {
        Ok(Ok(response)) => {
            let html = response.into_string().unwrap_or_default();

            // --- HTML转纯文本（CPU密集型，也交给线程池）---
            let text = tokio::task::spawn_blocking(move || html_to_text(&html))
                .await
                .unwrap_or_default();

            let content_len = text.len();

            // --- 写入文件（I/O阻塞操作）---
            let write_result = tokio::task::spawn_blocking(move || {
                fs::write(&filepath, &text)
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
        Ok(Err(error)) => {
            eprintln!("  请求失败 {}: {}", name, error);
            (name.to_string(), elapsed_ms, false, 0)
        }
        Err(_) => {
            // spawn_blocking 本身失败（线程池关闭等极端情况）
            eprintln!("  spawn_blocking失败: {}", name);
            (name.to_string(), elapsed_ms, false, 0)
        }
    }
}

/// 启动所有协程，用信号量控制最大并发数
///
/// 为什么要限制并发？
/// 同时发起33个HTTP请求可能被服务器限流/防火墙拦截。
/// 信号量就像一个"通行证"，最多只允许指定数量的协程同时执行。
async fn run_async(
    output_dir: &PathBuf,
    concurrency: usize,
) -> Vec<(String, f64, bool, usize)> {
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut handles = Vec::new();

    for school in SCHOOLS {
        let semaphore = semaphore.clone();
        let dir = output_dir.clone();

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap(); // 拿到通行证才继续
            crawl_one_async(school, &dir).await
            // _permit 离开作用域时自动释放，下一个协程就能拿到
        });

        handles.push(handle);
    }

    // 等待所有协程完成，收集结果
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

    // --- 准备输出目录 ---
    let output_dir = {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        dir.pop();
        dir.push("Docs");
        dir.push("高校名称和官方网站");
        dir
    };
    fs::create_dir_all(&output_dir).expect("无法创建输出目录");

    let concurrency = 20; // 最大并发数：同时最多20个请求在飞

    println!("========================================");
    println!("  基于协程的爬虫 (Async/Coro-based Crawler)");
    println!("========================================");
    println!("学校总数: {}", SCHOOLS.len());
    println!("最大并发: {}", concurrency);
    println!("输出目录: {}", output_dir.display());
    println!();

    // --- 启动所有协程 ---
    let results = run_async(&output_dir, concurrency).await;

    let total_time = start_total.elapsed();
    let mem_after = current_rss_kb();

    // --- 打印每个学校的结果 ---
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
