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
    /// 生成该结果的关注参数签名（行业id+自定义关键词），用于缓存命中判断
    #[serde(default)]
    pub params: String,
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
/// 重点关注模式的专属缓存（存的是过滤/搜索后的最终视图 + 参数签名）
pub const FOCUS_CACHE_KEY: &str = "news_cache_focus";

/// 读取缓存（离线兜底）
pub fn load_cache(db: &Db) -> Option<(HotNewsResult, String)> {
    load_cache_key(db, CACHE_KEY)
}

pub fn load_cache_key(db: &Db, key: &str) -> Option<(HotNewsResult, String)> {
    let raw = db.get_setting(key).ok().flatten()?;
    let at = db.get_setting(format!("{key}_at").as_str()).ok().flatten().unwrap_or_default();
    serde_json::from_str::<HotNewsResult>(&raw).ok().map(|r| (r, at))
}

/// 写缓存
pub fn save_cache(db: &Db, result: &HotNewsResult) {
    save_cache_key(db, CACHE_KEY, result)
}

pub fn save_cache_key(db: &Db, key: &str, result: &HotNewsResult) {
    if result.items.is_empty() {
        return; // 空结果不覆盖旧缓存
    }
    if let Ok(json) = serde_json::to_string(result) {
        let _ = db.set_setting(key, &json);
        let _ = db.set_setting(format!("{key}_at").as_str(), &result.fetched_at);
    }
}

// ───────────────────────── 关键词互联网搜索（重点关注不一定是热点） ─────────────────────────
//
// 用户反馈：自定义关键词（如「AI驱动开发」）在热搜榜里过滤几乎命中不了——热搜是大众榜。
// 这里用搜索引擎的网页结果做关键词资讯检索：每词一次请求（HTML 可解析、无需 Key、国内可达），
// 按「标题命中 > 摘要命中 + 新近度」打分排序（符合度高低）。
// 单点依赖会被反爬/网络波动打断（v1.0.x 只接了必应，用户反馈检索偶发为空），
// v1.1.1 起维护一个搜索源注册表：前一个源请求失败或解析不出结果时自动切换下一个。

/// 一个关键词搜索源。新增源只需在此登记（URL + 参数名 + 解析函数），
/// 超时、失败切换、相关度打分全部复用现有逻辑。
pub struct SearchEngine {
    pub id: &'static str,
    pub name: &'static str,
    /// 搜索页地址（不含查询参数）
    pub url: &'static str,
    /// 关键词参数名（bing: q / baidu: wd / sogou: query）
    pub query_key: &'static str,
    /// 附加固定参数（控制返回条数等）
    pub extra_query: &'static [(&'static str, &'static str)],
    /// 结果链接是相对路径时用它补全（搜狗返回 /link?url=...）
    pub link_base: &'static str,
    pub parse: fn(&str) -> Vec<SearchHit>,
}

/// 按优先级排序：必应（现行主源）→ 百度 → 搜狗
pub const SEARCH_ENGINES: &[SearchEngine] = &[
    SearchEngine {
        id: "bing",
        name: "必应",
        url: "https://cn.bing.com/search",
        query_key: "q",
        extra_query: &[("count", "20")],
        link_base: "",
        parse: parse_bing_results,
    },
    SearchEngine {
        id: "baidu",
        name: "百度",
        url: "https://www.baidu.com/s",
        query_key: "wd",
        extra_query: &[("rn", "20")],
        link_base: "",
        parse: parse_h3_results,
    },
    SearchEngine {
        id: "sogou",
        name: "搜狗",
        url: "https://www.sogou.com/web",
        query_key: "query",
        extra_query: &[("num", "20")],
        link_base: "https://www.sogou.com",
        parse: parse_h3_results,
    },
];

/// 上一次成功的搜索源下标（进程级记忆）：下一个关键词优先用上次成功的源，
/// 正常情况下每个关键词只发 1 次请求；该源挂了才顺延切换。
static LAST_GOOD_ENGINE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 相对链接补全（//开头 → https:；/开头 → 拼源站域名）
pub fn absolutize_url(url: &str, link_base: &str) -> String {
    let u = url.trim();
    if u.starts_with("//") {
        format!("https:{u}")
    } else if u.starts_with('/') && !link_base.is_empty() {
        format!("{}{}", link_base.trim_end_matches('/'), u)
    } else {
        u.to_string()
    }
}

/// 解析 h3 型搜索结果页（百度/搜狗）：每个 `<h3…><a href>标题</a></h3>` 视为一条结果，
/// 摘要取该链接之后到下一个 h3 之间的纯文本（去标签压空白，只影响新近度加成，容忍过采）。
/// 注：rust regex 不支持 lookahead，先收集全部 h3 链接再按位置切片。
pub fn parse_h3_results(html: &str) -> Vec<SearchHit> {
    let re =
        regex::Regex::new(r#"<h3[^>]*>\s*<a[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#).unwrap();
    let hits: Vec<regex::Captures> = re.captures_iter(html).collect();
    let mut out = Vec::new();
    for (i, c) in hits.iter().enumerate() {
        let title = strip_tags(&c[2]);
        if title.is_empty() {
            continue;
        }
        let start = c.get(0).unwrap().end();
        let end = hits
            .get(i + 1)
            .map(|n| n.get(0).unwrap().start())
            .unwrap_or_else(|| html.len().min(start + 2000));
        out.push(SearchHit {
            title,
            url: unescape_entities(c[1].trim()),
            snippet: strip_tags(&html[start..end]).chars().take(200).collect(),
        });
    }
    out
}

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

/// 单个关键词的互联网搜索（最多返回 max 条，相关度降序）。
/// 源冗余：从「上次成功的源」开始按注册表顺序逐个尝试，请求失败或解析不出结果
/// 自动切换下一个源；全部失败返回空（上层按无结果处理）。
pub async fn search_keyword(client: &reqwest::Client, keyword: &str, max: usize) -> Vec<NewsItem> {
    let kw = keyword.trim();
    if kw.is_empty() {
        return Vec::new();
    }
    let n = SEARCH_ENGINES.len();
    let start = LAST_GOOD_ENGINE.load(std::sync::atomic::Ordering::Relaxed).min(n - 1);
    for offset in 0..n {
        let idx = (start + offset) % n;
        let engine = &SEARCH_ENGINES[idx];
        let items = fetch_engine(client, engine, kw, max).await;
        if !items.is_empty() {
            LAST_GOOD_ENGINE.store(idx, std::sync::atomic::Ordering::Relaxed);
            return items;
        }
    }
    Vec::new()
}

/// 对单个源发起一次关键词搜索并解析打分（失败/被反爬拦截 → 空列表，交由上层切换）
async fn fetch_engine(
    client: &reqwest::Client,
    engine: &SearchEngine,
    kw: &str,
    max: usize,
) -> Vec<NewsItem> {
    let mut req = client
        .get(engine.url)
        .query(&[(engine.query_key, kw)])
        .query(engine.extra_query)
        .header("accept", "text/html")
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        );
    // 百度/搜狗对无 Referer 的请求更敏感，带上更像正常浏览
    if engine.id != "bing" {
        req = req.header("referer", engine.url);
    }
    let html = match req.send().await {
        Ok(r) => match r.text().await {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };
    let mut scored: Vec<(i64, NewsItem)> = (engine.parse)(&html)
        .into_iter()
        .map(|hit| {
            let url = absolutize_url(&hit.url, engine.link_base);
            let score = relevance_score(&hit.title, &hit.snippet, kw);
            (
                score,
                NewsItem {
                    title: hit.title,
                    url,
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

/// 批量搜索自定义关键词（最多取前 5 个词，控制总时长；单次失败跳过不阻断）。
/// 总时长预算 40s：源全挂时最坏 5 词 × 3 源 × 6s 会拖死前端请求，这里整体兜底。
pub async fn search_keywords(keywords: &[String], per_keyword: usize) -> Vec<NewsItem> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .connect_timeout(std::time::Duration::from_secs(4))
        .build()
        .unwrap_or_default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    let mut all: Vec<NewsItem> = Vec::new();
    for kw in keywords.iter().take(5) {
        if std::time::Instant::now() >= deadline {
            tracing::warn!("关键词搜索总时长超预算，剩余关键词跳过");
            break;
        }
        let hits = search_keyword(&client, kw, per_keyword).await;
        all.extend(hits);
    }
    // 全局按相关度降序（保持各词内部次序的稳定性用稳定排序）
    all.sort_by(|a, b| b.hot.unwrap_or(0).cmp(&a.hot.unwrap_or(0)));
    all
}

/// 抓取全部配置栏目（4s 超时，栏目间互不影响）
// ───────────────── 源冗余（v1.1.4）：60s 公共实例被限流（HTTP 429/CF 1027）后的兜底 ─────────────────
// 实测（2026-09-20）：60s.viki.moe 全量 429，任何抓取都失败只能吃缓存（用户反馈「一直显示本次抓取失败」）。
// 方案：每个栏目配置一个**直连上游**端点（各平台自家热搜接口，实测均可用、字段结构见 parse），
// 直连优先、60s 聚合实例降级兜底——它恢复后自动回到双保险。

/// 直连上游源：一个栏目一条。headers 是该源实测必需的请求头（微博对 Referer 敏感、
/// 知乎要用 App UA、B 站要 Referer），parse 从 JSON 里抠出 (标题, 链接, 热度)。
pub struct DirectSource {
    pub url: &'static str,
    pub headers: &'static [(&'static str, &'static str)],
    pub parse: fn(&serde_json::Value) -> Vec<(String, String, Option<i64>)>,
}

const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";

type ParseFn = fn(&serde_json::Value) -> Vec<(String, String, Option<i64>)>;

const H_WEIBO: &[(&str, &str)] = &[
    ("user-agent", BROWSER_UA),
    ("referer", "https://weibo.com"),
    ("accept", "application/json"),
];
const H_ZHIHU: &[(&str, &str)] = &[("user-agent", "osee2unifiedRelease/8.20.0")];
const H_TOUTIAO: &[(&str, &str)] = &[("user-agent", BROWSER_UA)];
const H_BILI: &[(&str, &str)] = &[
    ("user-agent", BROWSER_UA),
    ("referer", "https://www.bilibili.com"),
];
const H_DOUYIN: &[(&str, &str)] = &[("user-agent", BROWSER_UA)];

/// 微博热搜搜索页（直连源只给热词，链接按词构造到微博搜索）
fn weibo_link(word: &str) -> String {
    format!("https://s.weibo.com/weibo?q={}", urlencoding::encode(word))
}

fn parse_weibo(v: &serde_json::Value) -> Vec<(String, String, Option<i64>)> {
    v["data"]["realtime"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|it| {
                    let word = it["word"].as_str()?.trim().to_string();
                    if word.is_empty() {
                        return None;
                    }
                    Some((word, weibo_link(it["word"].as_str().unwrap_or("")), it["num"].as_i64()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_zhihu(v: &serde_json::Value) -> Vec<(String, String, Option<i64>)> {
    v["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|it| {
                    let title = it["target"]["title"].as_str()?.trim().to_string();
                    if title.is_empty() {
                        return None;
                    }
                    // api.zhihu.com 域名在浏览器打开会跳到对应问题页
                    let url = it["target"]["url"].as_str().unwrap_or_default().to_string();
                    Some((title, url, None))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_toutiao(v: &serde_json::Value) -> Vec<(String, String, Option<i64>)> {
    v["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|it| {
                    let title = it["Title"].as_str()?.trim().to_string();
                    if title.is_empty() {
                        return None;
                    }
                    let url = it["Url"].as_str().unwrap_or_default().to_string();
                    let hot = it["HotValue"].as_str().and_then(|s| s.parse::<i64>().ok());
                    Some((title, url, hot))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_bili(v: &serde_json::Value) -> Vec<(String, String, Option<i64>)> {
    v["data"]["trending"]["list"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|it| {
                    let kw = it["show_name"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .or_else(|| it["keyword"].as_str())?
                        .trim()
                        .to_string();
                    if kw.is_empty() {
                        return None;
                    }
                    let url = format!(
                        "https://search.bilibili.com/all?keyword={}",
                        urlencoding::encode(&kw)
                    );
                    Some((kw, url, it["heat_score"].as_i64()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_douyin(v: &serde_json::Value) -> Vec<(String, String, Option<i64>)> {
    v["word_list"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|it| {
                    let word = it["word"].as_str()?.trim().to_string();
                    if word.is_empty() {
                        return None;
                    }
                    let url = format!(
                        "https://www.douyin.com/search/{}",
                        urlencoding::encode(&word)
                    );
                    Some((word, url, it["hot_value"].as_i64()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 栏目 → 直连源注册表（与 CHANNELS 栏目 id 对应；新增栏目只改这里）
pub fn direct_source_for(channel: &str) -> Option<DirectSource> {
    let (url, headers, parse): (&str, &'static [(&'static str, &'static str)], ParseFn) = match channel {
        "weibo" => ("https://weibo.com/ajax/side/hotSearch", H_WEIBO, parse_weibo),
        "zhihu" => ("https://api.zhihu.com/topstory/hot-list?limit=50", H_ZHIHU, parse_zhihu),
        "toutiao" => (
            "https://www.toutiao.com/hot-event/hot-board/?origin=toutiao_pc",
            H_TOUTIAO,
            parse_toutiao,
        ),
        "bili" => (
            "https://api.bilibili.com/x/web-interface/search/square?limit=50",
            H_BILI,
            parse_bili,
        ),
        "douyin" => (
            "https://www.iesdouyin.com/web/api/v2/hotsearch/billboard/word/",
            H_DOUYIN,
            parse_douyin,
        ),
        _ => return None,
    };
    Some(DirectSource { url, headers, parse })
}

/// 抓一个栏目的直连上游：JSON 解析 + 字段清洗在 parse 里，这里只管请求与装配
async fn fetch_direct(
    client: &reqwest::Client,
    channel: &str,
    channel_name: &str,
    limit: usize,
) -> Option<Vec<NewsItem>> {
    let src = direct_source_for(channel)?;
    let mut req = client.get(src.url);
    for (k, v) in src.headers {
        req = req.header(*k, *v);
    }
    let text = match req.send().await {
        Ok(r) => r.text().await.ok()?,
        Err(_) => return None,
    };
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let items: Vec<NewsItem> = (src.parse)(&v)
        .into_iter()
        .take(limit)
        .map(|(title, url, hot)| NewsItem {
            title,
            url,
            hot,
            channel: channel.to_string(),
            channel_name: channel_name.to_string(),
        })
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

pub async fn fetch_channels(channels: &[String], limit: usize) -> (Vec<Vec<NewsItem>>, Vec<String>) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .connect_timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default();
    let mut groups = Vec::new();
    let mut errors = Vec::new();
    for ch in channels {
        let name = CHANNELS
            .iter()
            .find(|(k, _)| k == ch)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| ch.to_string());
        // 直连上游优先（60s 公共实例 2026-09 起限流，见模块注释）
        if let Some(items) = fetch_direct(&client, ch, &name, limit).await {
            groups.push(items);
            continue;
        }
        // 60s 聚合实例兜底
        let url = format!("{SOURCE_BASE}/{ch}");
        match client.get(&url).header("accept", "application/json").send().await {
            Ok(resp) => match resp.text().await {
                Ok(text) => {
                    let items = parse_channel(ch, &text);
                    if items.is_empty() {
                        errors.push(format!("{ch}: 直连与聚合源均为空"));
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
            params: String::new(),
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
        params: String::new(),
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
            params: String::new(),
        };
        save_cache(&db, &result);
        let (cached, at) = load_cache(&db).unwrap();
        assert_eq!(cached.items.len(), 1);
        assert_eq!(at, "2026-09-19 10:00:00");

        let empty = HotNewsResult { items: vec![], source: "live".into(), fetched_at: "x".into(), errors: vec![], stale: false, params: String::new() };
        save_cache(&db, &empty);
        let (cached2, _) = load_cache(&db).unwrap();
        assert_eq!(cached2.items.len(), 1, "空结果不应覆盖旧缓存");
    }
}

#[cfg(test)]
mod search_engine_tests {
    use super::*;

    #[test]
    fn 注册表_源id与参数名唯一() {
        let mut ids = SEARCH_ENGINES.iter().map(|e| e.id).collect::<Vec<_>>();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "搜索源 id 不得重复");
        // 首位必须是现行主源必应（老用户无感升级）
        assert_eq!(SEARCH_ENGINES[0].id, "bing");
        // 每个源的解析函数都要能拿出结果（用各自样例页校验，防登记错函数）
        for e in SEARCH_ENGINES {
            assert!(!e.url.is_empty() && !e.query_key.is_empty());
        }
    }

    #[test]
    fn h3解析_百度样例页() {
        let html = r#"<div class="result c-container"><h3 class="c-title t"><a href="http://www.baidu.com/link?url=abc123">AI<em>大模型</em>落地指南</a></h3><div class="c-abstract">3 分钟前发布的内容摘要</div></div>
        <div class="result"><h3 class="t"><a href="https://example.com/direct">直接链接的结果</a></h3><span class="c-color-text">2 小时前</span></div>
        <div><h3>没有链接的不算结果</h3></div>"#;
        let hits = parse_h3_results(html);
        assert_eq!(hits.len(), 2, "无链接的 h3 不应产出条目");
        assert_eq!(hits[0].title, "AI大模型落地指南", "标题内 <em> 高亮不该拆词");
        assert_eq!(hits[0].url, "http://www.baidu.com/link?url=abc123");
        assert!(hits[0].snippet.contains("3 分钟前"), "摘要用于新近度加成");
        assert_eq!(hits[1].url, "https://example.com/direct");
    }

    #[test]
    fn h3解析_搜狗相对链接补全() {
        let html = r#"<div class="vrwrap"><h3 class="vr-title"><a href="/link?url=xyz">搜狗结果标题</a></h3><div class="str_info">1 天前</div></div>"#;
        let hits = parse_h3_results(html);
        assert_eq!(hits.len(), 1);
        let engine = SEARCH_ENGINES.iter().find(|e| e.id == "sogou").unwrap();
        assert_eq!(absolutize_url(&hits[0].url, engine.link_base), "https://www.sogou.com/link?url=xyz");
    }

    #[test]
    fn 相对链接补全_各形态() {
        assert_eq!(absolutize_url("//cdn.example.com/a", ""), "https://cdn.example.com/a");
        assert_eq!(absolutize_url("/link?u=1", "https://www.sogou.com"), "https://www.sogou.com/link?u=1");
        assert_eq!(absolutize_url("https://a.b/c", ""), "https://a.b/c");
        assert_eq!(absolutize_url(" /x ", "https://e.com/"), "https://e.com/x");
    }

    #[test]
    fn 打分_跨源结果统一口径() {
        // 同一关键词在百度/必应解析出的结果用同一 relevance_score，保证多源合并排序不失真
        let kw = "新能源车";
        assert!(relevance_score("新能源车下乡政策发布", "", kw) > relevance_score("无关标题", "提到新能源车", kw));
    }
}

#[cfg(test)]
mod direct_source_tests {
    use super::*;

    #[test]
    fn 注册表_五个栏目全有直连源() {
        for (id, _) in CHANNELS {
            assert!(direct_source_for(id).is_some(), "栏目 {id} 缺直连源");
        }
        assert!(direct_source_for("不存在").is_none());
    }

    #[test]
    fn 微博解析_词与热度() {
        let v = serde_json::json!({
            "ok": 1,
            "data": {"realtime": [
                {"word": "长期不工作的人会失去什么", "num": 2126399},
                {"word": "传统豪车集体降价续命", "num": 1892222},
                {"word": "", "num": 1}
            ]}
        });
        let items = parse_weibo(&v);
        assert_eq!(items.len(), 2, "空词过滤");
        assert_eq!(items[0].0, "长期不工作的人会失去什么");
        assert_eq!(items[0].2, Some(2126399));
        assert!(items[0].1.starts_with("https://s.weibo.com/weibo?q="), "按词构造搜索链接");
    }

    #[test]
    fn 知乎解析_标题与问题链接() {
        let v = serde_json::json!({
            "data": [
                {"target": {"title": "字节跳动将飞书并入豆包意味着什么？", "url": "https://api.zhihu.com/questions/123"}, "detail_text": "521 万热度"},
                {"target": {"title": "", "url": ""}}
            ]
        });
        let items = parse_zhihu(&v);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].0, "字节跳动将飞书并入豆包意味着什么？");
        assert_eq!(items[0].1, "https://api.zhihu.com/questions/123");
    }

    #[test]
    fn 头条解析_标题链接与热度字符串() {
        let v = serde_json::json!({
            "data": [
                {"Title": "北大复旦校长接连发出警告", "Url": "https://www.toutiao.com/trending/123/", "HotValue": "9863351"},
                {"Title": " "},
                {"Title": "无热度值条目"}
            ]
        });
        let items = parse_toutiao(&v);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].2, Some(9863351), "HotValue 字符串转数字");
        assert_eq!(items[1].2, None);
    }

    #[test]
    fn B站解析_关键词条目() {
        let v = serde_json::json!({
            "code": 0,
            "data": {"trending": {"title": "bilibili热搜", "list": [
                {"keyword": "锐评IG战胜JDG晋级S赛", "show_name": "锐评IG战胜JDG晋级S赛", "heat_score": 1613343}
            ]}}
        });
        let items = parse_bili(&v);
        assert_eq!(items.len(), 1);
        assert!(items[0].1.starts_with("https://search.bilibili.com/all?keyword="));
        assert_eq!(items[0].2, Some(1613343));
    }

    #[test]
    fn 抖音解析_词与热度() {
        let v = serde_json::json!({
            "status_code": 0,
            "word_list": [
                {"word": "2026亚运会开幕式", "hot_value": 12171980},
                {"word": "布莱顿3:0完胜阿森纳", "hot_value": 9876543}
            ]
        });
        let items = parse_douyin(&v);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, "2026亚运会开幕式");
        assert_eq!(items[0].2, Some(12171980));
        assert!(items[0].1.starts_with("https://www.douyin.com/search/"));
    }

    #[test]
    fn 解析_非法JSON不炸返回空() {
        assert!(parse_weibo(&serde_json::Value::Null).is_empty());
        assert!(parse_toutiao(&serde_json::json!({"data": "不是数组"})).is_empty());
    }
}
