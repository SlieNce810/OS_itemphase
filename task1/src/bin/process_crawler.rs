// ========================================================================
// 基于进程的爬虫程序
// ========================================================================
// 核心思路：每爬一个学校，就启动一个操作系统子进程（调用 curl 命令）。
// 各子进程拥有独立的地址空间和内存，进程间互不干扰。
// 代价：创建进程开销大，但隔离性最好。
// ========================================================================
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

use crawler::common::{current_rss_kb, html_to_text, print_latency_stats, print_memory_stats};
use crawler::schools::{SchoolInfo, SCHOOLS};

/// 爬取单个学校的网页内容
///
/// 返回值：(耗时ms, 是否成功, 文本长度字节)
fn crawl_one(school: &SchoolInfo, output_dir: &PathBuf) -> (f64, bool, usize) {
    let start = Instant::now();

    let output = Command::new("curl")
        .arg("-s")                     // 静默模式，不输出进度条
        .arg("-L")                     // 自动跟随重定向（很多高校网站会跳转）
        .arg("--max-time").arg("30")   // 最多等30秒，防止卡死
        .arg("-A").arg("Mozilla/5.0 (compatible; Crawler/1.0)")  // 伪装浏览器
        .arg(&school.url)
        .stdout(Stdio::piped())        // 捕获下载内容
        .stderr(Stdio::null())         // 丢弃错误信息
        .output();

    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    match output {
        Ok(output) if output.status.success() => {
            // curl 成功：HTML 转纯文本，保存到文件
            let html = String::from_utf8_lossy(&output.stdout).to_string();
            let text = html_to_text(&html);
            let content_len = text.len();

            let filepath = output_dir.join(format!("{}.txt", school.name));
            if let Err(error) = fs::write(&filepath, &text) {
                eprintln!("  写入文件失败 {}: {}", school.name, error);
                (elapsed_ms, false, 0)
            } else {
                (elapsed_ms, true, content_len)
            }
        }
        _ => {
            // curl 失败：网络不通、超时、DNS解析失败等
            eprintln!("  curl请求失败: {}", school.name);
            (elapsed_ms, false, 0)
        }
    }
}

fn main() {
    let start_total = Instant::now();
    let mem_before = current_rss_kb();

    // --- 准备输出目录 ---
    let output_dir = {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        dir.pop(); // 退到项目根目录 OS_itemphase
        dir.push("Docs");
        dir.push("高校名称和官方网站");
        dir
    };
    fs::create_dir_all(&output_dir).expect("无法创建输出目录");

    println!("========================================");
    println!("  基于进程的爬虫 (Process-based Crawler)");
    println!("========================================");
    println!("学校总数: {}", SCHOOLS.len());
    println!("输出目录: {}", output_dir.display());
    println!();

    // --- 逐个爬取 ---
    let mut total_success = 0usize;
    let mut total_fail = 0usize;
    let mut latencies: Vec<f64> = Vec::new();

    for school in SCHOOLS {
        println!("正在爬取: {} ({})", school.name, school.url);

        let (latency, success, content_len) = crawl_one(school, &output_dir);

        if success {
            total_success += 1;
            println!("  ✓ 成功, 耗时 {:.0}ms, 文本长度 {} 字节", latency, content_len);
        } else {
            total_fail += 1;
            println!("  ✗ 失败, 耗时 {:.0}ms", latency);
        }

        latencies.push(latency);
    }

    let total_time = start_total.elapsed();
    let mem_after = current_rss_kb();

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
