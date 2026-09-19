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

/// 重点关注（行业资讯）注册表：id / 名称 / 关键词（标题命中任一即算关注内容）。
/// 与 CHANNELS 一样"自动生成"：新增行业只改这里，设置页与接口自动带出。
pub const FOCUS_TOPICS: &[(&str, &str, &[&str])] = &[
    ("ai", "AI", &["AI", "人工智能", "大模型", "AGI", "GPT", "OpenAI", "DeepSeek", "智谱", "算力", "机器人", "智能体"]),
    ("edu", "教育", &["教育", "高考", "中考", "高校", "大学", "考试", "开学", "双减", "留学", "招生"]),
    ("agri", "农业", &["农业", "粮食", "种植", "丰收", "乡村", "农产品", "耕地", "猪肉", "养殖"]),
    ("med", "医疗", &["医疗", "医院", "疫苗", "药品", "医保", "疾病", "健康", "集采"]),
    ("fin", "财经", &["财经", "股市", "A股", "金融", "央行", "利率", "基金", "经济", "GDP", "汇率"]),
    ("auto", "汽车", &["汽车", "新能源车", "电动车", "车企", "自动驾驶", "锂电"]),
    ("tech", "科技", &["科技", "芯片", "互联网", "手机", "发布会", "卫星", "航天", "5G", "鸿蒙"]),
    ("sport", "体育", &["体育", "足球", "篮球", "奥运", "冠军", "联赛", "世俱杯"]),
];

/// 重点关注 id 是否合法
pub fn valid_focus(id: &str) -> bool {
    FOCUS_TOPICS.iter().any(|(k, _, _)| *k == id)
}

/// 标题是否命中任意关键词（大小写不敏感；空关键词集合视为不命中）
pub fn title_matches(title: &str, keywords: &[String]) -> bool {
    let lower = title.to_lowercase();
    keywords
        .iter()
        .filter(|k| !k.trim().is_empty())
        .any(|k| lower.contains(&k.trim().to_lowercase()))
}

/// 按重点关注（预设行业 + 自定义关键词）过滤热点。
/// 预设 id 展开为其关键词，与自定义关键词合并；集合为空则原样返回。
pub fn filter_focus(items: Vec<NewsItem>, focus_ids: &[String], custom_keywords: &[String]) -> Vec<NewsItem> {
    let mut keywords: Vec<String> = Vec::new();
    for id in focus_ids {
        if let Some((_, _, kws)) = FOCUS_TOPICS.iter().find(|(k, _, _)| k == id) {
            keywords.extend(kws.iter().map(|s| s.to_string()));
        }
    }
    keywords.extend(custom_keywords.iter().cloned());
    if keywords.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|n| title_matches(&n.title, &keywords))
        .collect()
}

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

// ───────────────────────── 关键词互联网搜索（重点关注不一定是热点） ─────────────────────────
//
// 用户反馈：自定义关键词（如「AI驱动开发」）在热搜榜里过滤几乎命中不了——热搜是大众榜。
// 这里用必应中国的网页搜索结果做关键词资讯检索：每词一次请求（HTML 可解析、无需 Key、国内可达），
// 按「标题命中 > 摘要命中 + 新近度」打分排序（符合度高低）。

const SEARCH_URL: &str = "https://cn.bing.com/search";

/// 搜索结果条目（相关度得分借用 NewsItem.hot 字段承载）
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// 还原 HTML 实体（搜索结果标题/摘要里常见）
pub fn unescape_entities(s: &str) -> String {
    let mut out = s.to_string();
    let pairs = [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&nbsp;", " "),
        ("&ensp;", " "),
        ("&emsp;", " "),
        ("&#0183;", "·"),
        ("&#183;", "·"),
        ("&middot;", "·"),
        ("&hellip;", "…"),
    ];
    for (from, to) in pairs {
        out = out.replace(from, to);
    }
    // 数字实体 &#NNNN;
    if out.contains("&#") {
        let re = regex::Regex::new(r"&#(\d{2,5});").unwrap();
        out = re
            .replace_all(&out, |c: &regex::Captures| {
                char::from_u32(c[1].parse().unwrap_or(0))
                    .map(|ch| ch.to_string())
                    .unwrap_or_default()
            })
            .to_string();
    }
    out
}

/// 去掉 HTML 标签并压缩空白
fn strip_tags(s: &str) -> String {
    // 标签直接删除而非替换为空格：标题内的 <em> 高亮不该把词拆开（AI<em>驱动</em>开发 ≠ AI 驱动 开发）
    let re = regex::Regex::new(r"<[^>]*>").unwrap();
    let plain = re.replace_all(s, "").to_string();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 解析必应搜索结果页（<li class="b_algo">…<h2><a href>标题</a></h2>…摘要 …</li>）
pub fn parse_bing_results(html: &str) -> Vec<SearchHit> {
    let re = regex::Regex::new(
        r#"<li class="b_algo"[\s\S]*?<h2[^>]*><a[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a></h2>([\s\S]*?)</li>"#,
    )
    .unwrap();
    re.captures_iter(html)
        .filter_map(|c| {
            let url = c[1].trim().to_string();
            let title = strip_tags(&c[2]);
            let snippet = strip_tags(&c[3]);
            if title.is_empty() {
                return None;
            }
            Some(SearchHit {
                title,
                url: unescape_entities(&url),
                snippet,
            })
        })
        .collect()
}

/// 相关度得分：标题完整命中 > 摘要命中，另有新近度加成（必应摘要在结果前面带「N 分钟前/小时前/天前」）
pub fn relevance_score(title: &str, snippet: &str, keyword: &str) -> i64 {
    let kw = keyword.trim().to_lowercase();
    if kw.is_empty() {
        return 0;
    }
    let t = title.to_lowercase();
    let s = snippet.to_lowercase();
    let mut score = 0;
    if t.contains(&kw) {
        score += 100;
    } else {
        // 关键词的所有字符都出现（乱序/间隔命中）也给基础分
        if kw.chars().all(|ch| t.contains(ch)) {
            score += 20;
        }
    }
    if s.contains(&kw) {
        score += 30;
    }
    score += if snippet.contains("分钟前") {
        40
    } else if snippet.contains("小时前") {
        35
    } else if snippet.contains("天前") {
        20
    } else {
        0
    };
    score
}

/// 单个关键词的互联网搜索（最多返回 max 条，相关度降序）
pub async fn search_keyword(client: &reqwest::Client, keyword: &str, max: usize) -> Vec<NewsItem> {
    let kw = keyword.trim();
    if kw.is_empty() {
        return Vec::new();
    }
    let resp = client
        .get(SEARCH_URL)
        .query(&[("q", kw), ("count", "20")])
        .header("accept", "text/html")
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .send()
        .await;
    let html = match resp {
        Ok(r) => match r.text().await {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };
    let mut scored: Vec<(i64, NewsItem)> = parse_bing_results(&html)
        .into_iter()
        .take(max.max(10))
        .map(|hit| {
            let score = relevance_score(&hit.title, &hit.snippet, kw);
            (
                score,
                NewsItem {
                    title: hit.title,
                    url: hit.url,
                    hot: Some(score),
                    channel: "search".into(),
                    // 用关键词作为来源标识：用户能看到该条是哪个关注词搜出来的
                    channel_name: kw.to_string(),
                },
            )
        })
        .filter(|(score, item)| *score > 0 && !item.url.is_empty())
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .map(|(_, item)| item)
        .take(max)
        .collect()
}

/// 批量搜索自定义关键词（最多取前 5 个词，控制总时长；单次失败跳过不阻断）
pub async fn search_keywords(keywords: &[String], per_keyword: usize) -> Vec<NewsItem> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .connect_timeout(std::time::Duration::from_secs(4))
        .build()
        .unwrap_or_default();
    let mut all: Vec<NewsItem> = Vec::new();
    for kw in keywords.iter().take(5) {
        let hits = search_keyword(&client, kw, per_keyword).await;
        all.extend(hits);
    }
    // 全局按相关度降序（保持各词内部次序的稳定性用稳定排序）
    all.sort_by(|a, b| b.hot.unwrap_or(0).cmp(&a.hot.unwrap_or(0)));
    all
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
    fn 重点关注_注册表与id校验() {
        assert!(FOCUS_TOPICS.len() >= 5);
        assert!(valid_focus("ai") && valid_focus("edu") && valid_focus("agri"));
        assert!(!valid_focus("crypto"));
        for (id, name, kws) in FOCUS_TOPICS {
            assert!(!id.is_empty() && !name.is_empty() && !kws.is_empty());
        }
    }

    #[test]
    fn 重点关注_标题命中大小写不敏感() {
        assert!(title_matches("DeepSeek 发布新模型", &["deepseek".into()]));
        assert!(title_matches("农业丰收在望", &["农业".into(), "AI".into()]));
        assert!(!title_matches("某地举办美食节", &["农业".into()]));
        assert!(!title_matches("任意标题", &[]), "空关键词不命中");
        assert!(!title_matches("任意标题", &["  ".into()]), "空白关键词不命中");
    }

    #[test]
    fn 重点关注_预设与自定义关键词合并过滤() {
        let mk = |title: &str| NewsItem {
            title: title.into(),
            url: String::new(),
            hot: None,
            channel: "weibo".into(),
            channel_name: "微博热搜".into(),
        };
        let items = vec![mk("AI 大模型开源"), mk("高考人数创新高"), mk("秋粮丰收"), mk("某明星婚礼"), mk("AI驱动开发流程走红")];
        // 仅预设：edu
        let got = filter_focus(items.clone(), &["edu".into()], &[]);
        assert_eq!(got.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["高考人数创新高"]);
        // 预设 + 自定义关键字
        let got = filter_focus(items, &["agri".into()], &["AI驱动".into()]);
        let titles: Vec<&str> = got.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, vec!["秋粮丰收", "AI驱动开发流程走红"]);
        // 空配置：原样返回
        let items = vec![mk("任意")];
        assert_eq!(filter_focus(items.clone(), &[], &[]).len(), 1);
    }

    #[test]
    fn 实体还原_常见与数字实体() {
        assert_eq!(unescape_entities("A&amp;B &lt;C&gt;"), "A&B <C>");
        assert_eq!(unescape_entities("&#39;引用&#39;"), "'引用'");
        assert_eq!(unescape_entities("1 天前&ensp;&#0183;&ensp;正文"), "1 天前 · 正文");
        assert_eq!(unescape_entities("&#26085;&#26412;"), "日本");
    }

    #[test]
    fn 必应结果解析_提取标题链接摘要() {
        let html = r#"<html><body><ol>
            <li class="b_algo"><h2><a href="https://example.com/a?q=1&amp;x=2">AI<em>驱动</em>开发落地</a></h2>
              <div>1 小时前&ensp;&#0183;&ensp;本文介绍 AI 驱动开发流程的实践。</div></li>
            <li class="b_algo"><h2><a href="/relative/path">无 http 前缀的结果</a></h2>
              <div>3 天前 相关讨论。</div></li>
            <li class="b_algo"><h2><a href="https://example.com/empty"></a></h2><div>空标题应过滤</div></li>
        </ol></body></html>"#;
        let hits = parse_bing_results(html);
        assert_eq!(hits.len(), 2, "空标题应被过滤");
        assert_eq!(hits[0].title, "AI驱动开发落地");
        assert_eq!(hits[0].url, "https://example.com/a?q=1&x=2");
        assert!(hits[0].snippet.contains("AI 驱动开发流程"));
        assert_eq!(relevance_score(&hits[0].title, &hits[0].snippet, "AI驱动开发") >= 130,
                   true, "标题命中100 + 摘要命中30 + 小时前35");
        // 相关度：标题命中 > 仅摘要命中
        assert!(relevance_score("AI驱动开发指南", "", "AI驱动开发")
            > relevance_score("其他标题", "讲讲AI驱动开发", "AI驱动开发"));
        // 新近度：分钟前 > 小时前 > 天前 > 无时间（标题不含关键词时不给命中分）
        assert!(relevance_score("标题甲", "5 分钟前", "关键词丙") > relevance_score("标题甲", "2 小时前", "关键词丙"));
        assert!(relevance_score("标题甲", "2 小时前", "关键词丙") > relevance_score("标题甲", "3 天前", "关键词丙"));
        assert!(relevance_score("标题甲", "昨天发生的事", "关键词丙") == 0);
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
