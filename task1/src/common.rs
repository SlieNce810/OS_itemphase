/// 公共工具函数模块
/// 三个爬虫程序共享的辅助函数都在这里
use regex::Regex;

/// 获取当前进程的常驻内存大小（RSS, 单位KB）
///
/// 原理：读取 Linux 的 /proc/self/status 文件，解析 VmRSS 行
/// 坑点：此方法仅在 Linux 上有效，macOS/Windows 上会返回 0
pub fn current_rss_kb() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                if line.starts_with("VmRSS:") {
                    // VmRSS 行格式如 "VmRSS:    12345 kB"
                    line.split_whitespace().nth(1).and_then(|value| value.parse().ok())
                } else {
                    None
                }
            })
        })
        .unwrap_or(0) // 读取失败时返回0，不崩溃
}

/// 将 HTML 网页源码转成干净纯文本
///
/// 处理步骤（按顺序）：
///   1. 移除 script/style/noscript 标签及内容（JS、CSS对文本分析无用）
///   2. 移除所有剩余 HTML 标签
///   3. 替换 HTML 实体（&amp; &nbsp; &#160; 等）为空格
///   4. 合并多余空白和换行
pub fn html_to_text(html: &str) -> String {
    // --- 第1步：移除脚本、样式、noscript 块 ---
    // 为什么不写成一个正则 `<(script|style|noscript)...?</\1>`？
    // 因为 Rust 的 regex crate 不支持反向引用 \1，只能拆成三个独立正则
    let regex_script = regex::RegexBuilder::new(r"<script[^>]*?>[\s\S]*?</script>")
        .case_insensitive(true)
        .build()
        .unwrap();
    let regex_style = regex::RegexBuilder::new(r"<style[^>]*?>[\s\S]*?</style>")
        .case_insensitive(true)
        .build()
        .unwrap();
    let regex_noscript = regex::RegexBuilder::new(r"<noscript[^>]*?>[\s\S]*?</noscript>")
        .case_insensitive(true)
        .build()
        .unwrap();

    let without_script = regex_script.replace_all(html, "");
    let without_style = regex_style.replace_all(&without_script, "");
    let without_noscript = regex_noscript.replace_all(&without_style, "");

    // --- 第2步：移除所有 HTML 标签 ---
    let regex_tag = Regex::new(r"<[^>]*>").unwrap();
    let without_tags = regex_tag.replace_all(&without_noscript, "");

    // --- 第3步：替换 HTML 实体为空格 ---
    // 实体有三种写法：&amp;（名称）、&#160;（十进制）、&#xa0;（十六进制）
    let regex_entity = Regex::new(r"&[a-zA-Z]+;|&#\d+;|&#x[0-9a-fA-F]+;").unwrap();
    let without_entities = regex_entity.replace_all(&without_tags, " ");

    // --- 第4步：合并多余空白和换行 ---
    let regex_whitespace = Regex::new(r"[ \t]+").unwrap();
    let compact = regex_whitespace.replace_all(&without_entities, " ");
    let regex_newlines = Regex::new(r"\n{3,}").unwrap();
    let result = regex_newlines.replace_all(&compact, "\n\n");

    result.trim().to_string()
}

/// 打印延迟分布统计
pub fn print_latency_stats(latencies: &[f64]) {
    if latencies.is_empty() { return; }

    let mut sorted = latencies.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let sum: f64 = sorted.iter().sum();
    let average = sum / sorted.len() as f64;
    let minimum = sorted.first().unwrap();
    let maximum = sorted.last().unwrap();
    let median = sorted[sorted.len() / 2];

    // P95：95% 的请求在此时间内完成
    let p95_index = ((sorted.len() as f64) * 0.95).ceil() as usize - 1;
    let p95 = sorted[p95_index.min(sorted.len() - 1)];

    println!();
    println!("延迟分布 (ms):");
    println!("  最小值:  {:.2}", minimum);
    println!("  平均值:  {:.2}", average);
    println!("  中位数:  {:.2}", median);
    println!("  P95:     {:.2}", p95);
    println!("  最大值:  {:.2}", maximum);
}

/// 打印内存开销信息
pub fn print_memory_stats(before_kb: usize, after_kb: usize) {
    let increase = after_kb.saturating_sub(before_kb);
    println!();
    println!("内存开销 (RSS):");
    println!("  爬取前:  {} KB ({:.1} MB)", before_kb, before_kb as f64 / 1024.0);
    println!("  爬取后:  {} KB ({:.1} MB)", after_kb, after_kb as f64 / 1024.0);
    println!("  增量:    {} KB ({:.1} MB)", increase, increase as f64 / 1024.0);
}
