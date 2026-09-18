//! 今日热点：抓取互联网热点新闻（我的简报旁的「今日热点」卡片数据源）
//!
//! 数据源：60s 开源 API（github.com/vikiboss/60s，`https://60s.viki.moe/v2/{channel}`），
//! 返回 `{code:200, data:[{title, link, hot_value?}]}`。栏目（渠道）清单由此模块的注册表自动生成，
//! 用户只需在设置里多选栏目与显示条数，无需手填地址。
//!
//! 可靠性设计：
//! - 请求 4s 超时，逐栏目失败互不影响（部分成功也照常展示）
//! - 成功结果整体写入 settings（`news_cache`），源站全挂/离线时回退缓存并标记 `stale`
//! - 解析与合并为纯函数（单元测试覆盖），网络层薄封装

use crate::db::Db;
use serde::{Deserialize, Serialize};

/// 可用栏目（注册表即"自动生成的栏目数据"：新增栏目只改这里）
pub const CHANNELS: &[(&str, &str)] = &[
    ("weibo", "微博热搜"),
    ("zhihu", "知乎热榜"),
    ("douyin", "抖音热点"),
    ("toutiao", "头条热点"),
    ("bili", "B站热搜"),
];

const SOURCE_BASE: &str = "https://60s.viki.moe/v2";

/// 栏目清单是否合法（设置保存时校验）
pub fn valid_channel(id: &str) -> bool {
    CHANNELS.iter().any(|(k, _)| *k == id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    pub title: String,
    pub url: String,
    /// 热度值（源站字段缺失时为 null，仅排序展示用）
    pub hot: Option<i64>,
    /// 来源栏目 id（weibo/zhihu/...）
    pub channel: String,
    /// 来源栏目名（微博热搜）
    pub channel_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotNewsResult {
    pub items: Vec<NewsItem>,
    /// live=本次实抓 cache=回退缓存
    pub source: String,
    /// 数据抓取时刻（缓存时为缓存写入时刻）
    pub fetched_at: String,
    /// 各栏目失败原因（全成功为空）
    pub errors: Vec<String>,
    /// true=源站抓取失败后回退的历史数据（UI 据此提示「非实时」）
    #[serde(default)]
    pub stale: bool,
}

/// 解析单栏目响应（60s API：`{code:200, data:[{title, link, hot_value|hot_value_desc?}]}`）
pub fn parse_channel(channel_id: &str, body: &str) -> Vec<NewsItem> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        data: serde_json::Value,
    }
    let channel_name = CHANNELS
        .iter()
        .find(|(k, _)| *k == channel_id)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| channel_id.to_string());
    let Ok(raw) = serde_json::from_str::<Raw>(body) else {
        return Vec::new();
    };
    let Some(arr) = raw.data.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            let title = v.get("title")?.as_str()?.trim().to_string();
            if title.is_empty() {
                return None;
            }
            let url = v
                .get("link")
                .and_then(|x| x.as_str())
                .or_else(|| v.get("url").and_then(|x| x.as_str()))
                .unwrap_or_default()
                .to_string();
            // 热度：数字直接用；字符串（如 "1,234"）去千分位后解析
            let hot = v
                .get("hot_value")
                .or_else(|| v.get("hot_value_desc"))
                .and_then(|x| {
                    x.as_i64()
                        .or_else(|| x.as_str().and_then(|s| s.replace(',', "").parse().ok()))
                });
            Some(NewsItem {
                title,
                url,
                hot,
                channel: channel_id.to_string(),
                channel_name: channel_name.clone(),
            })
        })
        .collect()
}

/// 合并各栏目并按条数上限截断（轮转取样，避免单栏目霸屏）
pub fn merge_items(groups: Vec<Vec<NewsItem>>, limit: usize) -> Vec<NewsItem> {
    let mut out: Vec<NewsItem> = Vec::with_capacity(limit);
    let mut idx = vec![0usize; groups.len()];
    while out.len() < limit {
        let mut progressed = false;
        for (gi, g) in groups.iter().enumerate() {
            if out.len() >= limit {
                break;
            }
            let i = idx[gi];
            if i < g.len() {
                out.push(g[i].clone());
                idx[gi] = i + 1;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    out
}

const CACHE_KEY: &str = "news_cache";
const CACHE_AT_KEY: &str = "news_cache_at";

/// 读取缓存（离线兜底）
pub fn load_cache(db: &Db) -> Option<(HotNewsResult, String)> {
    let raw = db.get_setting(CACHE_KEY).ok().flatten()?;
    let at = db.get_setting(CACHE_AT_KEY).ok().flatten().unwrap_or_default();
    serde_json::from_str::<HotNewsResult>(&raw).ok().map(|r| (r, at))
}

/// 写缓存
pub fn save_cache(db: &Db, result: &HotNewsResult) {
    if result.items.is_empty() {
        return; // 空结果不覆盖旧缓存
    }
    if let Ok(json) = serde_json::to_string(result) {
        let _ = db.set_setting(CACHE_KEY, &json);
        let _ = db.set_setting(CACHE_AT_KEY, &result.fetched_at);
    }
}

/// 抓取全部配置栏目（4s 超时，栏目间互不影响）
pub async fn fetch_channels(channels: &[String], limit: usize) -> (Vec<Vec<NewsItem>>, Vec<String>) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .connect_timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default();
    let mut groups = Vec::new();
    let mut errors = Vec::new();
    for ch in channels {
        let url = format!("{SOURCE_BASE}/{ch}");
        match client.get(&url).header("accept", "application/json").send().await {
            Ok(resp) => match resp.text().await {
                Ok(text) => {
                    let items = parse_channel(ch, &text);
                    if items.is_empty() {
                        errors.push(format!("{ch}: 响应解析为空"));
                    }
                    groups.push(items);
                }
                Err(e) => {
                    errors.push(format!("{ch}: {e}"));
                    groups.push(Vec::new());
                }
            },
            Err(e) => {
                errors.push(format!("{ch}: {e}"));
                groups.push(Vec::new());
            }
        }
    }
    (groups, errors)
}

/// 从源站抓取并写缓存；全失败时回退缓存（stale 标记）
pub async fn fetch_hot_news(db: &Db, channels: &[String], limit: usize) -> HotNewsResult {
    let (groups, errors) = fetch_channels(channels, limit).await;
    let merged = merge_items(groups, limit);
    let fetched_at = crate::db::now_string();
    if !merged.is_empty() {
        let result = HotNewsResult {
            items: merged,
            source: "live".into(),
            fetched_at,
            errors,
            stale: false,
        };
        save_cache(db, &result);
        return result;
    }
    // 全部失败：回退缓存（标记 stale，UI 提示非实时）
    if let Some((mut cached, at)) = load_cache(db) {
        cached.source = "cache".into();
        cached.fetched_at = at;
        cached.stale = true;
        let mut errs = vec!["本次抓取失败，展示的是最近一次成功的数据".to_string()];
        errs.extend(cached.errors.clone());
        cached.errors = errs;
        // 缓存也要按当前上限截断
        cached.items.truncate(limit);
        return cached;
    }
    HotNewsResult {
        items: Vec::new(),
        source: "none".into(),
        fetched_at,
        errors,
        stale: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 栏目注册表_含三大栏目且id合法() {
        assert!(CHANNELS.len() >= 3);
        assert!(valid_channel("weibo") && valid_channel("zhihu") && valid_channel("douyin"));
        assert!(!valid_channel("weibo2"));
        // id 与名称一一对应
        for (id, name) in CHANNELS {
            assert!(!id.is_empty() && !name.is_empty());
        }
    }

    #[test]
    fn 解析60s响应_提取标题链接与热度() {
        let body = r#"{"code":200,"message":"ok","data":[
            {"title":"热点一","link":"https://s.weibo.com/weibo?q=a","hot_value":837190},
            {"title":"热点二","link":"https://s.weibo.com/weibo?q=b","hot_value":"1,234"},
            {"title":"","link":""},
            {"title":"无链接热点","hot_value":10}
        ]}"#;
        let items = parse_channel("weibo", body);
        assert_eq!(items.len(), 3, "空标题应被过滤");
        assert_eq!(items[0].title, "热点一");
        assert_eq!(items[0].url, "https://s.weibo.com/weibo?q=a");
        assert_eq!(items[0].hot, Some(837190));
        assert_eq!(items[1].hot, Some(1234), "字符串热度（含千分位）应可解析");
        assert_eq!(items[2].url, "", "缺链接允许为空");
        assert_eq!(items[0].channel_name, "微博热搜");
    }

    #[test]
    fn 解析异常响应_返回空而不崩溃() {
        assert!(parse_channel("weibo", "not json").is_empty());
        assert!(parse_channel("weibo", r#"{"code":500}"#).is_empty());
        assert!(parse_channel("weibo", r#"{"data":null}"#).is_empty());
    }

    #[test]
    fn 合并_轮转取样且按上限截断() {
        let g1: Vec<NewsItem> = (1..=5)
            .map(|i| NewsItem { title: format!("a{i}"), url: String::new(), hot: None, channel: "weibo".into(), channel_name: "微博热搜".into() })
            .collect();
        let g2: Vec<NewsItem> = (1..=5)
            .map(|i| NewsItem { title: format!("b{i}"), url: String::new(), hot: None, channel: "zhihu".into(), channel_name: "知乎热榜".into() })
            .collect();
        let merged = merge_items(vec![g1.clone(), g2], 4);
        let titles: Vec<String> = merged.iter().map(|i| i.title.clone()).collect();
        assert_eq!(titles, vec!["a1", "b1", "a2", "b2"], "应轮转取样避免单栏目霸屏");
        assert_eq!(merge_items(vec![g1], 100).len(), 5, "上限大于总量时全部保留");
    }

    #[test]
    fn 缓存_空结果不覆盖旧缓存() {
        let db = Db::open_memory().unwrap();
        let result = HotNewsResult {
            items: vec![NewsItem { title: "t".into(), url: String::new(), hot: None, channel: "weibo".into(), channel_name: "微博热搜".into() }],
            source: "live".into(),
            fetched_at: "2026-09-19 10:00:00".into(),
            errors: vec![],
            stale: false,
        };
        save_cache(&db, &result);
        let (cached, at) = load_cache(&db).unwrap();
        assert_eq!(cached.items.len(), 1);
        assert_eq!(at, "2026-09-19 10:00:00");

        let empty = HotNewsResult { items: vec![], source: "live".into(), fetched_at: "x".into(), errors: vec![], stale: false };
        save_cache(&db, &empty);
        let (cached2, _) = load_cache(&db).unwrap();
        assert_eq!(cached2.items.len(), 1, "空结果不应覆盖旧缓存");
    }
}
