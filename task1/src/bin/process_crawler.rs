/// 基于进程的爬虫程序
/// 为每个学校URL启动一个子进程（curl）进行HTTP请求，
/// 各进程独立运行，拥有独立的地址空间和内存。
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

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

    // 合并多余空白
    let ws_re = regex::Regex::new(r"[ \t]+").unwrap();
    let compact = ws_re.replace_all(&text, " ");
    let nl_re = regex::Regex::new(r"\n{3,}").unwrap();
    let result = nl_re.replace_all(&compact, "\n\n");

    result.trim().to_string()
}

/// 爬取单个学校URL（在子进程中执行）
fn crawl_one(school: &SchoolInfo, output_dir: &PathBuf) -> (f64, bool, usize) {
    let start = Instant::now();

    // 使用curl命令下载网页内容
    let output = Command::new("curl")
        .arg("-s")                     // silent mode
        .arg("-L")                     // follow redirects
        .arg("--max-time").arg("30")   // 30秒超时
        .arg("-A").arg("Mozilla/5.0 (compatible; Crawler/1.0)")  // User-Agent
        .arg(&school.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    match output {
        Ok(output) if output.status.success() => {
            let html = String::from_utf8_lossy(&output.stdout).to_string();
            let text = html_to_text(&html);
            let content_len = text.len();

            // 保存纯文本到文件（文件名为学校中文名称）
            let filepath = output_dir.join(format!("{}.txt", school.name));
            if let Err(e) = fs::write(&filepath, &text) {
                eprintln!("  写入文件失败 {}: {}", school.name, e);
                (elapsed_ms, false, 0)
            } else {
                (elapsed_ms, true, content_len)
            }
        }
        _ => {
            eprintln!("  curl请求失败: {}", school.name);
            (elapsed_ms, false, 0)
        }
    }
}

fn main() {
    let start_total = Instant::now();
    let mem_before = current_rss_kb();

    // 确定输出目录
    let output_dir = {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        dir.pop(); // 回到项目根目录 OS_itemphase
        dir.push("Docs");
        dir
    };
    fs::create_dir_all(&output_dir).expect("无法创建输出目录");

    println!("========================================");
    println!("  基于进程的爬虫 (Process-based Crawler)");
    println!("========================================");
    println!("学校总数: {}", SCHOOLS.len());
    println!("输出目录: {}", output_dir.display());
    println!();

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
