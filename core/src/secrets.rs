//! 安全存储：本地 token、JWT 密钥落盘（0600），AI API Key 存系统安全存储（keyring）

use anyhow::{Context, Result};
use rand::RngCore;
use std::path::Path;

fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

/// 本地模式的访问 token（写盘 0600；桌面端注入，浏览器本机访问免登录）
pub fn load_or_create_token(data_dir: &Path) -> Result<String> {
    let path = data_dir.join(".token");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return Ok(s);
        }
    }
    let token = random_hex(32);
    std::fs::create_dir_all(data_dir).ok();
    std::fs::write(&path, &token).context("写入 token 失败")?;
    restrict_permissions(&path);
    Ok(token)
}

/// JWT 签名密钥
pub fn load_or_create_jwt_secret(data_dir: &Path) -> Result<String> {
    let path = data_dir.join(".jwt");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return Ok(s);
        }
    }
    let secret = random_hex(48);
    std::fs::create_dir_all(data_dir).ok();
    std::fs::write(&path, &secret).context("写入 jwt 密钥失败")?;
    restrict_permissions(&path);
    Ok(secret)
}

/// 尽力收紧文件权限（Windows 依赖 ACL，失败不阻断）
fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

const KEYRING_SERVICE: &str = "mindmate";
/// AI API Key 的凭据名
pub const SECRET_AI_KEY: &str = "ai_api_key";
/// 邮件（SMTP）密码 / 授权码 的凭据名
pub const SECRET_SMTP_PASSWORD: &str = "smtp_password";
/// Telegram Bot Token 的凭据名
pub const SECRET_TELEGRAM_TOKEN: &str = "telegram_bot_token";

/// 写入系统安全存储（Windows Credential Manager / macOS Keychain）。
/// 系统安全存储不可用时返回错误交由调用方提示，绝不明文落库。
pub fn save_secret(name: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, name)?;
    entry
        .set_password(value)
        .with_context(|| format!("写入系统安全存储失败（{name}）"))
}

/// 读取系统安全存储中的凭据（不存在返回 None）
pub fn load_secret(name: &str) -> Result<Option<String>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, name)?;
    match entry.get_password() {
        Ok(v) if !v.is_empty() => Ok(Some(v)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("读取系统安全存储失败（{name}）: {e}")),
    }
}

pub fn delete_secret(name: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, name)?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("删除安全存储条目失败（{name}）: {e}")),
    }
}

/// 是否已配置某凭据
pub fn has_secret(name: &str) -> bool {
    matches!(load_secret(name), Ok(Some(_)))
}

// ── AI Key 便捷封装 ──

/// 环境变量覆盖：设置后用它的值当作 AI Key（**空字符串表示"没有 Key"**）。
///
/// 两个用途：
///   1) 自动化验收必须**确定、离线**——系统钥匙串是全局的，临时测试服务即使用独立数据目录
///      也照样读到用户的真实 Key，于是测试会去打真实上游 API；上游一旦限流/网络波动，
///      断言就随机失败（这正是长期存在的"偶发 175/176"的根因）。测试服务设
///      `MINDMATE_AI_KEY=`（空值）即可确定性走本地降级路径。
///   2) 服务器/容器部署可直接从环境注入 Key，不依赖桌面钥匙串。
pub const ENV_AI_KEY: &str = "MINDMATE_AI_KEY";

pub fn save_api_key(key: &str) -> Result<()> {
    save_secret(SECRET_AI_KEY, key)
}
pub fn load_api_key() -> Result<Option<String>> {
    if let Ok(v) = std::env::var(ENV_AI_KEY) {
        let v = v.trim().to_string();
        return Ok(if v.is_empty() { None } else { Some(v) });
    }
    load_secret(SECRET_AI_KEY)
}
pub fn delete_api_key() -> Result<()> {
    delete_secret(SECRET_AI_KEY)
}

/// 访问密码哈希（Argon2id）
pub fn hash_password(password: &str) -> Result<String> {
    use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
    use argon2::Argon2;
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("密码哈希失败: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    use argon2::Argon2;
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_key_环境变量可覆盖且空值表示未配置() {
        // 自动化验收依赖这个语义：测试服务设 MINDMATE_AI_KEY="" → 视为未配置 Key，
        // 走确定性的本地降级路径，不会拿用户真实 Key 去打真实上游 API。
        let saved = std::env::var(ENV_AI_KEY).ok();

        std::env::set_var(ENV_AI_KEY, "sk-from-env");
        assert_eq!(load_api_key().unwrap().as_deref(), Some("sk-from-env"));

        std::env::set_var(ENV_AI_KEY, "");
        assert_eq!(load_api_key().unwrap(), None, "空值必须表示未配置");
        assert!(!matches!(load_api_key(), Ok(Some(_))));

        std::env::set_var(ENV_AI_KEY, "   ");
        assert_eq!(load_api_key().unwrap(), None, "纯空白也视为未配置");

        match saved {
            Some(v) => std::env::set_var(ENV_AI_KEY, v),
            None => std::env::remove_var(ENV_AI_KEY),
        }
    }
}
