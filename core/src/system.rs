//! 系统集成小工具：用系统默认浏览器打开外链。
//!
//! 背景：设置页的「去申请 Key」需要跳出应用打开厂商页面，但 Tauri 的 WebView
//! 不提供打开外部浏览器的能力（要么引入 opener 插件、多一个原生依赖）。
//! 桌面端本来就有本机内核在跑，让内核直接调起系统浏览器最省事，
//! 且**只在本地模式开放**：局域网/服务器模式下这个接口会被拒绝，
//! 避免被远端「借手」调起本机进程。

use std::process::Command;

/// 校验外链：只放行 http/https、长度有限、不含空白/控制字符与引号类字符。
///
/// 虽然调用时不经过 shell（参数化 spawn，不存在命令注入），
/// 仍然做白名单校验——这里返回的字符串最终会被交给系统协议处理器，
/// 正规厂商申请页也不会带引号/尖括号/反斜杠这类字符。
pub fn validate_external_url(url: &str) -> Result<String, String> {
    let u = url.trim();
    if u.is_empty() {
        return Err("链接为空".into());
    }
    if u.chars().count() > 512 {
        return Err("链接过长".into());
    }
    if !u.starts_with("https://") && !u.starts_with("http://") {
        return Err("只支持 http/https 链接".into());
    }
    if u.chars()
        .any(|c| c.is_whitespace() || c.is_control() || "\"'`<>\\^".contains(c))
    {
        return Err("链接含空白或非法字符".into());
    }
    Ok(u.to_string())
}

/// 调起系统默认浏览器（Windows: rundll32 协议处理器 / macOS: open / Linux: xdg-open）
pub fn open_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        // 不用 cmd /c start：避免任何 shell 解析，参数整体作为协议处理器入参
        Command::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 外链校验_放行常见厂商申请页() {
        for u in [
            "https://open.bigmodel.cn/usercenter/apikeys",
            "https://platform.deepseek.com/api_keys",
            "https://ollama.com/download",
            "http://127.0.0.1:11434/",
        ] {
            assert_eq!(validate_external_url(u).unwrap(), u);
        }
        // 前后空白会被去掉（复制来的链接常带空格）
        assert_eq!(
            validate_external_url("  https://ollama.com/download  ").unwrap(),
            "https://ollama.com/download"
        );
    }

    #[test]
    fn 外链校验_拒绝非http与可疑输入() {
        for bad in [
            "",
            "   ",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "ms-settings:",
            "ftp://example.com",
            "https://example.com/a b",
            "https://example.com/\nrm -rf",
            "https://example.com/\"quoted\"",
        ] {
            assert!(
                validate_external_url(bad).is_err(),
                "应拒绝：{bad:?}"
            );
        }
        // 超长链接拒绝
        let long = format!("https://example.com/{}", "a".repeat(600));
        assert!(validate_external_url(&long).is_err());
    }
}
