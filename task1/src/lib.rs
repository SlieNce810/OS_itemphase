// 库 crate 的根文件
// 作用：将公共模块暴露给三个 bin 爬虫程序共享
// 如果没有这个文件，每个 bin 无法通过 "use crawler::schools" 引入共享数据
pub mod schools;
pub mod common;
