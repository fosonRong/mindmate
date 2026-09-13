//! 开机自启（Windows 注册表 / macOS LaunchAgent / Linux autostart）
//!
//! 不依赖第三方插件：自己实现便于给出明确的错误提示，并且可以用单元测试验证
//! 「写入 → 读回 → 移除」全流程（Windows 下使用当前用户的 Run 键，无需管理员权限）。

use anyhow::Result;
use std::path::Path;

/// 自启项名称（Windows 注册表值名 / Linux 桌面文件名 / macOS plist 标签）
pub const ENTRY_NAME: &str = "Mindmate";

/// 当前可执行文件路径
pub fn current_exe() -> Result<std::path::PathBuf> {
    std::env::current_exe().map_err(|e| anyhow::anyhow!("无法获取程序路径：{e}"))
}

// ───────────────────────── Windows ─────────────────────────

#[cfg(windows)]
mod platform {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

    /// 通过 reg.exe 读写注册表（避免额外依赖与管理员权限问题）
    fn reg(args: &[&str]) -> Result<(bool, String)> {
        let out = Command::new("reg")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| anyhow::anyhow!("调用注册表命令失败：{e}"))?;
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok((out.status.success(), text))
    }

    pub fn is_enabled(entry: &str) -> Result<bool> {
        let (ok, text) = reg(&["query", RUN_KEY, "/v", entry])?;
        Ok(ok && text.contains(entry))
    }

    pub fn enable(entry: &str, exe: &Path) -> Result<()> {
        // 加引号避免路径含空格被截断
        let value = format!("\"{}\"", exe.display());
        let (ok, _text) = reg(&["add", RUN_KEY, "/v", entry, "/t", "REG_SZ", "/d", &value, "/f"])?;
        if !ok {
            // 文案本地化不可靠，用读回结果给出可读错误
            if !is_enabled(entry)? {
                anyhow::bail!("写入启动项失败：注册表命令返回失败（可能被安全软件拦截）");
            }
        }
        // 回读校验，确保真的写进去了
        if !is_enabled(entry)? {
            anyhow::bail!("启动项写入后校验失败（可能被安全软件拦截）");
        }
        Ok(())
    }

    pub fn disable(entry: &str) -> Result<()> {
        // reg.exe 在「值不存在」时也会返回失败且输出为本地化（GBK）文本，
        // 因此不以文案判断，一律以读回状态为准（天然幂等）
        let _ = reg(&["delete", RUN_KEY, "/v", entry, "/f"]);
        if is_enabled(entry)? {
            anyhow::bail!("移除启动项失败（仍检测到自启配置，可能被安全软件锁定）");
        }
        Ok(())
    }
}

// ───────────────────────── macOS ─────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::io::Write;

    fn plist_path(entry: &str) -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("com.mindmate.{entry}.plist"))
    }

    pub fn is_enabled(entry: &str) -> Result<bool> {
        Ok(plist_path(entry).exists())
    }

    pub fn enable(entry: &str, exe: &Path) -> Result<()> {
        let path = plist_path(entry);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.mindmate.{entry}</string>
  <key>ProgramArguments</key><array><string>{}</string></array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
            exe.display()
        );
        let mut f = std::fs::File::create(&path)?;
        f.write_all(content.as_bytes())?;
        Ok(())
    }

    pub fn disable(entry: &str) -> Result<()> {
        let path = plist_path(entry);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

// ───────────────────────── Linux ─────────────────────────

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::*;
    use std::io::Write;

    fn desktop_path(entry: &str) -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(home)
            .join(".config/autostart")
            .join(format!("{}.desktop", entry.to_lowercase()))
    }

    pub fn is_enabled(entry: &str) -> Result<bool> {
        Ok(desktop_path(entry).exists())
    }

    pub fn enable(entry: &str, exe: &Path) -> Result<()> {
        let path = desktop_path(entry);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let content = format!(
            "[Desktop Entry]\nType=Application\nName={entry}\nExec={}\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        let mut f = std::fs::File::create(&path)?;
        f.write_all(content.as_bytes())?;
        Ok(())
    }

    pub fn disable(entry: &str) -> Result<()> {
        let path = desktop_path(entry);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// 当前是否已开启开机自启
pub fn is_enabled(entry: &str) -> Result<bool> {
    platform::is_enabled(entry)
}

/// 开启：把当前可执行文件写入系统自启项
pub fn enable(entry: &str) -> Result<()> {
    let exe = current_exe()?;
    platform::enable(entry, &exe)
}

/// 关闭
pub fn disable(entry: &str) -> Result<()> {
    platform::disable(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 使用独立测试项名，避免污染真实自启配置
    const TEST_ENTRY: &str = "MindmateSelfTest";

    #[test]
    fn 自启开关_写入读回移除全流程() {
        // 初始应为关闭
        let _ = disable(TEST_ENTRY);
        assert!(!is_enabled(TEST_ENTRY).unwrap(), "测试前应为关闭状态");

        enable(TEST_ENTRY).unwrap();
        assert!(is_enabled(TEST_ENTRY).unwrap(), "开启后应读取为已启用");

        // 幂等：重复开启不报错
        enable(TEST_ENTRY).unwrap();
        assert!(is_enabled(TEST_ENTRY).unwrap());

        disable(TEST_ENTRY).unwrap();
        assert!(!is_enabled(TEST_ENTRY).unwrap(), "关闭后应读取为未启用");

        // 幂等：重复关闭不报错
        disable(TEST_ENTRY).unwrap();
    }

    #[test]
    fn 当前可执行文件路径可获取() {
        let exe = current_exe().unwrap();
        assert!(exe.exists(), "可执行文件应存在：{}", exe.display());
    }
}
