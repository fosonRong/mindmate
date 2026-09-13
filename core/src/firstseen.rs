//! 首见证据（老用户识别埋点 · 商业化第二期 B 方案的前置）
//!
//! 第二期要给"商业化上线前就已使用"的老用户赠送 12 个月 Pro，而这份判定
//! **必须在第一期就写下证据**——数据一旦没有记录，事后无法补造。
//!
//! 三处互相印证（取最早值）：
//!   1. `settings` 表：`first_seen_at` / `first_seen_version` / `install_id`
//!   2. 数据目录 `.first_seen` 文件：JSON + 用本机 `jwt_secret` 做的 HS256 签名（防手改）
//!   3. 数据库最早一条记录（`nodes.created_at`）—— 既有数据天然具备
//!
//! 设计取舍见 docs/商业化方案.md 3.2.1：这是"送福利"而非"防线"，因此
//! 证据缺失时**不拒绝**，交由服务端兜底发放（宁滥勿缺）。

use crate::db::{now_string, Db};
use anyhow::{Context, Result};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 证据文件名
pub const FIRST_SEEN_FILE: &str = ".first_seen";
pub const KEY_FIRST_SEEN_AT: &str = "first_seen_at";
pub const KEY_FIRST_SEEN_VERSION: &str = "first_seen_version";
pub const KEY_INSTALL_ID: &str = "install_id";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FirstSeen {
    /// 首次使用时间（YYYY-MM-DD HH:MM:SS，本机时区）
    pub at: String,
    /// 写下证据时的程序版本
    pub version: String,
    /// 安装标识（随机 16 字节 hex，用于区分不同安装）
    pub install_id: String,
    /// 证据来源：earliest-record（据最早记录回填）/ now（本次首启）/ settings
    pub source: String,
    /// 是否通过签名校验（文件证据被篡改时为 false）
    pub signature_valid: bool,
}

/// 签名载荷（只签关键字段）
#[derive(Debug, Serialize, Deserialize)]
struct Claim {
    at: String,
    ver: String,
    iid: String,
}

fn to_claim(v: &FirstSeen) -> Claim {
    Claim {
        at: v.at.clone(),
        ver: v.version.clone(),
        iid: v.install_id.clone(),
    }
}

fn sign(value: &FirstSeen, secret: &str) -> Result<String> {
    Ok(encode(
        &Header::new(Algorithm::HS256),
        &to_claim(value),
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

fn verify(token: &str, secret: &str) -> Option<Claim> {
    // 注意：jsonwebtoken 默认要求 exp/审计声明，而这里的载荷是"数据"而非会话令牌，
    // 不含 exp —— 必须显式关闭这些校验，否则验签永远失败（曾因此让 signatureValid 恒为 false）。
    let mut validation = Validation::new(Algorithm::HS256);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    validation.validate_aud = false;
    decode::<Claim>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .ok()
    .map(|d| d.claims)
}

fn new_install_id() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FIRST_SEEN_FILE)
}

/// 读取文件证据（含签名校验）；不存在或损坏返回 None
pub fn read_file(data_dir: &Path, secret: &str) -> Option<(FirstSeen, bool)> {
    let raw = std::fs::read_to_string(file_path(data_dir)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let sig = value.get("sig")?.as_str()?.to_string();
    let claim: Claim = serde_json::from_value(value.clone()).ok()?;
    let ok = verify(&sig, secret)
        .map(|c| c.at == claim.at && c.ver == claim.ver && c.iid == claim.iid)
        .unwrap_or(false);
    Some((
        FirstSeen {
            at: claim.at,
            version: claim.ver,
            install_id: claim.iid,
            source: "file".into(),
            signature_valid: ok,
        },
        ok,
    ))
}

fn write_file(data_dir: &Path, value: &FirstSeen, secret: &str) -> Result<()> {
    std::fs::create_dir_all(data_dir).ok();
    let sig = sign(value, secret)?;
    let mut obj = serde_json::to_value(to_claim(value))?;
    if let Some(map) = obj.as_object_mut() {
        map.insert("sig".into(), serde_json::Value::String(sig));
    }
    let text = serde_json::to_string_pretty(&obj)?;
    std::fs::write(file_path(data_dir), text).context("写入 .first_seen 失败")?;
    Ok(())
}

/// 首次运行时埋点（幂等：重复调用不会改动已记录的时间）
///
/// 回填策略：若本机已有更早的记录（升级到本版本的老用户），取最早记录时间作为首次使用时间，
/// 这样即便埋点是在升级后才引入的，老用户依然能被正确识别。
pub fn ensure(db: &Db, data_dir: &Path, secret: &str, version: &str) -> Result<FirstSeen> {
    // 1) settings 中的既有证据
    let mut at = db.get_setting(KEY_FIRST_SEEN_AT)?;
    let mut source = "settings";
    if at.is_none() {
        // 2) 回填：取数据库最早记录时间；没有记录则用当前时间
        let earliest = db.earliest_record_at()?;
        let (value, src) = match earliest {
            Some(t) => (t, "earliest-record"),
            None => (now_string(), "now"),
        };
        db.set_setting(KEY_FIRST_SEEN_AT, &value)?;
        at = Some(value);
        source = src;
        tracing::info!("首次使用时间已记录：{}（来源 {source}）", at.clone().unwrap_or_default());
    }
    let at = at.unwrap_or_else(now_string);

    // 安装标识
    let install_id = match db.get_setting(KEY_INSTALL_ID)? {
        Some(v) if !v.is_empty() => v,
        _ => {
            let v = new_install_id();
            db.set_setting(KEY_INSTALL_ID, &v)?;
            v
        }
    };

    // 版本只记录一次（首次写入，不随后续升级变化）
    if db.get_setting(KEY_FIRST_SEEN_VERSION)?.is_none() {
        db.set_setting(KEY_FIRST_SEEN_VERSION, version)?;
    }
    let first_version = db
        .get_setting(KEY_FIRST_SEEN_VERSION)?
        .unwrap_or_else(|| version.to_string());

    let value = FirstSeen {
        at: at.clone(),
        version: first_version,
        install_id,
        source: source.into(),
        signature_valid: true,
    };

    // 文件证据：只在缺失时写入（保留最初那份），已存在则只做签名校验
    match read_file(data_dir, secret) {
        Some((file_value, ok)) => {
            // 文件里若更早（例如用户先用了绿色版），以更早者为准
            if file_value.at < at {
                db.set_setting(KEY_FIRST_SEEN_AT, &file_value.at)?;
                return Ok(FirstSeen {
                    at: file_value.at,
                    signature_valid: ok,
                    ..value
                });
            }
            Ok(FirstSeen {
                signature_valid: ok,
                ..value
            })
        }
        None => {
            write_file(data_dir, &value, secret)?;
            Ok(value)
        }
    }
}

/// 供接口/排查使用：读取当前首见证据（不改写任何内容）
pub fn current(db: &Db, data_dir: &Path, secret: &str, version: &str) -> Result<FirstSeen> {
    let at = db
        .get_setting(KEY_FIRST_SEEN_AT)?
        .or_else(|| db.earliest_record_at().ok().flatten())
        .unwrap_or_else(now_string);
    let version_seen = db
        .get_setting(KEY_FIRST_SEEN_VERSION)?
        .unwrap_or_else(|| version.to_string());
    let install_id = db.get_setting(KEY_INSTALL_ID)?.unwrap_or_default();
    let (signature_valid, file_at) = match read_file(data_dir, secret) {
        Some((v, ok)) => (ok, Some(v.at)),
        None => (false, None),
    };
    let at = match file_at {
        Some(f) if f < at => f,
        _ => at,
    };
    Ok(FirstSeen {
        at,
        version: version_seen,
        install_id,
        source: if file_path(data_dir).exists() { "file" } else { "settings" }.into(),
        signature_valid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewNode;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mindmate-fs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const SECRET: &str = "test-secret";

    #[test]
    fn 埋点_签名可自校验通过() {
        // 回归：验证必须是"能通过"的 —— 之前因 jsonwebtoken 默认要求 exp 声明，
        // 验签恒失败，导致接口里 signatureValid 永远是 false（真机实测才发现）。
        let dir = tmp_dir("verify-ok");
        let db = Db::open_memory().unwrap();
        let v = ensure(&db, &dir, SECRET, "1.0.0").unwrap();
        let (file_value, ok) = read_file(&dir, SECRET).unwrap();
        assert!(ok, "自己写下的证据必须能通过验签");
        assert_eq!(file_value.at, v.at);
        assert_eq!(file_value.install_id, v.install_id);
        // 换一个密钥（模拟被别的安装的文件）应验不过
        assert!(read_file(&dir, "other-secret").map(|(_, ok)| ok).unwrap_or(false) == false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 埋点_首次运行写入证据且幂等() {
        let dir = tmp_dir("idem");
        let db = Db::open_memory().unwrap();
        let first = ensure(&db, &dir, SECRET, "1.0.0").unwrap();
        assert_eq!(first.source, "now");
        assert!(!first.install_id.is_empty());
        assert!(file_path(&dir).exists(), ".first_seen 文件应被创建");

        // 第二次调用不得改动首次时间与安装标识
        let again = ensure(&db, &dir, SECRET, "1.1.0").unwrap();
        assert_eq!(again.at, first.at, "重复调用不应改动首次时间");
        assert_eq!(again.install_id, first.install_id);
        assert_eq!(again.version, "1.0.0", "首见版本只记录一次");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 埋点_升级老用户按最早记录回填() {
        let dir = tmp_dir("backfill");
        let db = Db::open_memory().unwrap();
        // 用户在半年前就记录过（模拟本埋点引入之前的老用户）
        db.create_node(NewNode {
            content: "很久以前的一条记录".into(),
            date: Some("2026-03-01".into()),
            tags: vec![],
            todo_id: None,
        })
        .unwrap();
        {
            let conn = db.lock();
            conn.execute(
                "UPDATE nodes SET created_at='2026-03-01 09:00:00' WHERE id=1",
                [],
            )
            .unwrap();
        }
        let v = ensure(&db, &dir, SECRET, "1.0.0").unwrap();
        assert_eq!(v.at, "2026-03-01 09:00:00", "应回填为最早记录时间");
        assert_eq!(v.source, "earliest-record");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 埋点_文件被篡改时签名校验失败() {
        let dir = tmp_dir("tamper");
        let db = Db::open_memory().unwrap();
        let v = ensure(&db, &dir, SECRET, "1.0.0").unwrap();

        // 手工改掉 at 字段（伪造"更早"的使用时间）
        let raw = std::fs::read_to_string(file_path(&dir)).unwrap();
        let mut json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        json["at"] = serde_json::Value::String("2020-01-01 00:00:00".into());
        std::fs::write(file_path(&dir), serde_json::to_string_pretty(&json).unwrap()).unwrap();

        let (file_value, ok) = read_file(&dir, SECRET).unwrap();
        assert!(!ok, "篡改后签名必须校验失败");
        assert_eq!(file_value.at, "2020-01-01 00:00:00");
        // current() 会把该状态如实上报（供服务端风控判断）
        let cur = current(&db, &dir, SECRET, "1.0.0").unwrap();
        assert!(!cur.signature_valid);
        // 但 settings 中的原始证据保持不变
        assert_eq!(db.get_setting(KEY_FIRST_SEEN_AT).unwrap().unwrap(), v.at);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 埋点_绿色版更早则取更早者() {
        let dir = tmp_dir("earlier");
        let db = Db::open_memory().unwrap();
        // 先手写一份更早的文件证据（模拟用户先用过绿色版）
        let older = FirstSeen {
            at: "2025-12-01 08:00:00".into(),
            version: "0.9.0".into(),
            install_id: "abcd".into(),
            source: "file".into(),
            signature_valid: true,
        };
        write_file(&dir, &older, SECRET).unwrap();

        let v = ensure(&db, &dir, SECRET, "1.0.0").unwrap();
        assert_eq!(v.at, "2025-12-01 08:00:00", "应取更早的文件证据");
        assert_eq!(
            db.get_setting(KEY_FIRST_SEEN_AT).unwrap().unwrap(),
            "2025-12-01 08:00:00"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
