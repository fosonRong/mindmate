//! HTTP API：REST + SSE + 静态资源托管 + 认证
//!
//! 桌面端与浏览器端走完全相同的接口（技术设计文档 §2.2）。

use crate::ai::{self, AiConfig, ChatMsg};
use crate::db::*;
use crate::{AppContext, Event};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event as SseEvent, KeepAlive, KeepAliveStream, Sse},
        IntoResponse, Json, Response,
    },
    routing::{delete, get, patch, post},
    Router,
};
use chrono::Datelike;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    services::ServeDir,
};

/// 统一响应体
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResp<T> {
    pub code: i32,
    pub message: String,
    pub data: Option<T>,
}

impl<T: Serialize> ApiResp<T> {
    pub fn ok(data: T) -> Json<Self> {
        Json(Self {
            code: 0,
            message: "ok".into(),
            data: Some(data),
        })
    }
}

pub type ApiResult<T> = Result<Json<ApiResp<T>>, ApiError>;

/// 错误 → HTTP 状态码 + 业务码
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: i32,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: i32, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, 4001, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, 4004, msg)
    }
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, 4002, msg)
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, 5000, msg)
    }
    pub fn ai(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, 4102, msg)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::internal(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "code": self.code,
            "message": self.message,
            "data": serde_json::Value::Null,
        }));
        (self.status, body).into_response()
    }
}

/// 内嵌前端资源解析器：path → (Content-Type, bytes)
pub type AssetResolver = Arc<dyn Fn(&str) -> Option<(String, Vec<u8>)> + Send + Sync>;

/// 构建全部路由
pub fn build_router(ctx: Arc<AppContext>) -> Router {
    build_router_with_assets(ctx, None)
}

/// 构建路由（可注入内嵌前端资源，桌面端使用；浏览器模式用 web_dir）
pub fn build_router_with_assets(ctx: Arc<AppContext>, assets: Option<AssetResolver>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut app = Router::new()
        .route("/api/v1/healthz", get(healthz))
        // 认证
        .route("/api/v1/auth/status", get(auth_status))
        .route("/api/v1/auth/login", post(auth_login))
        .route("/api/v1/auth/set-password", post(auth_set_password))
        // 节点
        .route("/api/v1/nodes", post(create_node).get(list_nodes))
        .route("/api/v1/nodes/range", get(list_nodes_range))
        .route("/api/v1/nodes/{id}", patch(update_node).delete(delete_node))
        .route("/api/v1/nodes/search", get(search_nodes))
        .route("/api/v1/tags", get(list_tags))
        // 统计
        .route("/api/v1/stats/daily", get(stats_daily))
        .route("/api/v1/stats/period", get(stats_period))
        .route("/api/v1/stats/monthly", get(stats_monthly))
        // 待办
        .route("/api/v1/todos", post(create_todo).get(list_todos))
        .route(
            "/api/v1/todos/{id}",
            patch(update_todo).delete(delete_todo),
        )
        .route("/api/v1/todos/{id}/complete", post(complete_todo))
        .route("/api/v1/todos/{id}/reopen", post(reopen_todo))
        .route("/api/v1/todos/schedule", get(schedule_for_date))
        .route("/api/v1/todos/suggest", post(suggest_schedule))
        // 设置
        .route("/api/v1/settings", get(all_settings).put(update_settings))
        .route("/api/v1/settings/{key}", get(get_setting))
        // 模板
        .route("/api/v1/templates", get(all_templates))
        .route("/api/v1/templates/{type}", get(get_template).put(set_template))
        // AI
        .route("/api/v1/ai/presets", get(ai_presets))
        .route("/api/v1/ai/config", get(ai_config).post(ai_save_config))
        .route("/api/v1/ai/test", post(ai_test))
        .route("/api/v1/ai/ollama", get(ai_ollama_probe))
        // 系统集成：用默认浏览器打开外链（仅本地模式）
        .route("/api/v1/system/open-url", post(system_open_url))
        // 今日热点：栏目清单（自动生成）+ 热点抓取（缓存降级）
        .route("/api/v1/news/channels", get(news_channels))
        .route("/api/v1/news/hot", get(news_hot))
        .route("/api/v1/ai/report", post(ai_report))
        .route("/api/v1/ai/brief", post(ai_brief))
        .route("/api/v1/ai/goodnight", post(ai_goodnight))
        .route("/api/v1/ai/review", post(ai_review))
        .route("/api/v1/ai/chat", post(ai_chat))
        .route("/api/v1/ai/replan", post(ai_replan))
        .route("/api/v1/ai/tag", post(ai_tag))
        // 报告
        .route("/api/v1/reports", get(list_reports))
        .route("/api/v1/reports/{id}", delete(delete_report))
        // 聊天
        .route("/api/v1/chat", get(list_chat).delete(clear_chat))
        // 成就
        .route("/api/v1/achievements", get(list_achievements))
        .route("/api/v1/achievements/check", post(check_achievements_api))
        // 安装信息 / 首见证据（商业化二期老用户识别用）
        .route("/api/v1/install", get(install_info))
        // 数据
        .route("/api/v1/data/export", get(data_export))
        .route("/api/v1/data/import", post(data_import))
        .route("/api/v1/data/export/markdown", get(data_export_markdown))
        // 推送渠道（FR-4.10）
        .route("/api/v1/push/config", get(push_config).post(push_save_config))
        .route("/api/v1/push/test", post(push_test))
        // 事件流
        .route("/api/v1/stream/events", get(stream_events))
        .layer(cors)
        .layer(CompressionLayer::new())
        .with_state(ctx.clone());

    // 静态资源（桌面端与浏览器端共用同一份前端产物）
    if let Some(assets) = assets {
        // 内嵌资源（打包后单二进制自带前端，无需外部文件）
        app = app.fallback(move |req: axum::extract::Request| {
            let assets = assets.clone();
            async move { serve_embedded(&assets, req.uri().path()) }
        });
    } else if let Some(dir) = &ctx.cfg.web_dir {
        if dir.exists() {
            app = app.fallback_service(
                ServeDir::new(dir).not_found_service(ServeDir::new(dir.join("index.html"))),
            );
        }
    }
    app
}

/// 从内嵌资源返回文件；未命中时回退 index.html（SPA 路由）
///
/// 缓存策略（避免应用升级后 WebView/浏览器混用新旧资源）：
/// - `index.html`：`no-cache`，每次校验，保证拿到新的资源引用
/// - `/assets/*`（Vite 内容哈希命名）：`immutable` 长期缓存
fn serve_embedded(assets: &AssetResolver, path: &str) -> Response {
    let clean = path.trim_start_matches('/');
    let key = if clean.is_empty() { "index.html" } else { clean };
    let is_fallback = assets(key).is_none();
    let hit = assets(key).or_else(|| assets("index.html"));
    let served_key = if is_fallback { "index.html" } else { key };

    let cache_control = if served_key.starts_with("assets/") && served_key.contains('-') {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    match hit {
        Some((ctype, bytes)) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, ctype),
                (header::CACHE_CONTROL, cache_control.to_string()),
            ],
            bytes,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// 启动服务（返回实际端口）
pub async fn serve(ctx: Arc<AppContext>, port_override: Option<u16>) -> anyhow::Result<u16> {
    serve_with_assets(ctx, port_override, None).await
}

/// 启动服务（可注入内嵌前端资源）
pub async fn serve_with_assets(
    ctx: Arc<AppContext>,
    port_override: Option<u16>,
    assets: Option<AssetResolver>,
) -> anyhow::Result<u16> {
    let mut port = port_override.unwrap_or(ctx.cfg.port);
    // 端口冲突自动 +1（最多尝试 20 次）
    let mut listener = None;
    for _ in 0..20 {
        let addr = format!("{}:{}", ctx.cfg.bind_addr(), port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => {
                listener = Some(l);
                break;
            }
            Err(_) => port += 1,
        }
    }
    let listener = listener.ok_or_else(|| anyhow::anyhow!("无法绑定端口（17801-17820 均被占用）"))?;
    let actual_port = listener.local_addr()?.port();
    let app = build_router_with_assets(ctx.clone(), assets);
    tracing::info!(
        "智伴服务已启动: http://{}:{} (模式: {})",
        if ctx.cfg.bind_addr() == "0.0.0.0" { "127.0.0.1" } else { ctx.cfg.bind_addr() },
        actual_port,
        ctx.cfg.mode.as_str()
    );
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("HTTP 服务异常退出: {e}");
        }
    });
    Ok(actual_port)
}

// ───────────────────────── 认证 ─────────────────────────

#[derive(Deserialize)]
struct LoginReq {
    password: String,
}

fn extract_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

/// 校验：本地模式免登录；局域网/服务器模式需 Bearer token（本地 token 或 JWT）
fn ensure_auth(ctx: &AppContext, headers: &HeaderMap) -> Result<(), ApiError> {
    if !ctx.cfg.requires_login() {
        return Ok(());
    }
    let Some(token) = extract_token(headers) else {
        return Err(ApiError::unauthorized("未登录"));
    };
    if token == ctx.local_token {
        return Ok(());
    }
    // 校验 JWT
    use jsonwebtoken::{decode, DecodingKey, Validation};
    #[derive(Deserialize)]
    struct Claims {
        sub: String,
        exp: usize,
    }
    let v = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(ctx.jwt_secret.as_bytes()),
        &Validation::default(),
    );
    match v {
        Ok(_) => Ok(()),
        Err(_) => Err(ApiError::unauthorized("登录已过期，请重新登录")),
    }
}

async fn auth_status(State(ctx): State<Arc<AppContext>>) -> ApiResult<serde_json::Value> {
    Ok(ApiResp::ok(json!({
        "mode": ctx.cfg.mode.as_str(),
        "requiresLogin": ctx.cfg.requires_login(),
        "hasPassword": ctx.db.has_owner()?,
        "lanEnabled": ctx.cfg.lan_enabled,
    })))
}

async fn auth_login(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<LoginReq>,
) -> ApiResult<serde_json::Value> {
    let Some(hash) = ctx.db.owner_password_hash("owner")? else {
        return Err(ApiError::bad_request("尚未设置访问密码"));
    };
    if !crate::secrets::verify_password(&req.password, &hash) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            4201,
            "密码错误",
        ));
    }
    use jsonwebtoken::{encode, EncodingKey, Header};
    #[derive(Serialize)]
    struct Claims {
        sub: String,
        exp: usize,
    }
    let exp = (chrono::Utc::now() + chrono::Duration::days(30)).timestamp() as usize;
    let token = encode(
        &Header::default(),
        &Claims {
            sub: "owner".into(),
            exp,
        },
        &EncodingKey::from_secret(ctx.jwt_secret.as_bytes()),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(ApiResp::ok(json!({ "token": token })))
}

async fn auth_set_password(
    State(ctx): State<Arc<AppContext>>,
    Json(req): Json<LoginReq>,
) -> ApiResult<serde_json::Value> {
    if req.password.chars().count() < 6 {
        return Err(ApiError::bad_request("密码至少 6 位"));
    }
    let hash = crate::secrets::hash_password(&req.password)?;
    ctx.db.upsert_owner("owner", &hash)?;
    Ok(ApiResp::ok(json!({ "ok": true })))
}

async fn healthz(State(ctx): State<Arc<AppContext>>) -> ApiResult<serde_json::Value> {
    Ok(ApiResp::ok(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": ctx.cfg.mode.as_str(),
        "time": crate::reminder::now_string(),
    })))
}

// ───────────────────────── 节点 ─────────────────────────

async fn create_node(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(input): Json<NewNode>,
) -> ApiResult<Node> {
    ensure_auth(&ctx, &headers)?;
    if input.content.trim().is_empty() {
        return Err(ApiError::bad_request("记录内容不能为空"));
    }
    let node = ctx.db.create_node(input)?;
    ctx.bus.publish(Event::new(
        "node.created",
        serde_json::to_value(&node).unwrap_or_default(),
    ));
    // 成就评估（FR-6.2）：起步/连续/手速/超额/记录满月等
    crate::achievements::check_all(&ctx.db, &ctx.bus)?;
    Ok(ApiResp::ok(node))
}

async fn list_nodes(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<Node>> {
    ensure_auth(&ctx, &headers)?;
    let date = q
        .get("date")
        .cloned()
        .unwrap_or_else(crate::db::today_string);
    Ok(ApiResp::ok(ctx.db.list_nodes_by_date(&date)?))
}

async fn list_nodes_range(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<Node>> {
    ensure_auth(&ctx, &headers)?;
    let from = q.get("from").cloned().unwrap_or_else(crate::db::today_string);
    let to = q.get("to").cloned().unwrap_or_else(|| from.clone());
    Ok(ApiResp::ok(ctx.db.list_nodes_range(&from, &to)?))
}

async fn update_node(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(patch): Json<NodePatch>,
) -> ApiResult<Node> {
    ensure_auth(&ctx, &headers)?;
    let node = ctx
        .db
        .update_node(id, patch)?
        .ok_or_else(|| ApiError::not_found("记录不存在"))?;
    ctx.bus.publish(Event::new(
        "node.updated",
        serde_json::to_value(&node).unwrap_or_default(),
    ));
    Ok(ApiResp::ok(node))
}

async fn delete_node(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let ok = ctx.db.delete_node(id)?;
    ctx.bus.publish(Event::new("node.deleted", json!({ "id": id })));
    Ok(ApiResp::ok(json!({ "deleted": ok })))
}

async fn search_nodes(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<Node>> {
    ensure_auth(&ctx, &headers)?;
    let kw = q.get("q").cloned().unwrap_or_default();
    if kw.trim().is_empty() {
        return Ok(ApiResp::ok(vec![]));
    }
    Ok(ApiResp::ok(ctx.db.search_nodes(&kw, 50)?))
}

// ───────────────────────── 统计 ─────────────────────────

async fn stats_daily(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<DailyStats> {
    ensure_auth(&ctx, &headers)?;
    let date = q
        .get("date")
        .cloned()
        .unwrap_or_else(crate::db::today_string);
    Ok(ApiResp::ok(ctx.db.daily_stats(&date)?))
}

async fn stats_period(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<PeriodStats> {
    ensure_auth(&ctx, &headers)?;
    let today = crate::db::today_string();
    let from = q.get("from").cloned().unwrap_or_else(|| today.clone());
    let to = q.get("to").cloned().unwrap_or(today);
    Ok(ApiResp::ok(ctx.db.period_stats(&from, &to)?))
}

/// 月度小结（FR-6.4）
async fn stats_monthly(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<MonthlySummary> {
    ensure_auth(&ctx, &headers)?;
    let date = q
        .get("date")
        .cloned()
        .unwrap_or_else(crate::db::today_string);
    Ok(ApiResp::ok(ctx.db.monthly_summary(&date)?))
}

// ───────────────────────── 待办 ─────────────────────────

/// 循环待办补期：生成缺失的「下一期」实例并广播 todo.created（幂等，失败不阻断主流程）
fn publish_recurring_created(ctx: &Arc<AppContext>) -> anyhow::Result<()> {
    if let Ok(created) = ctx.db.ensure_recurring() {
        for t in created {
            ctx.bus.publish(Event::new(
                "todo.created",
                serde_json::to_value(&t).unwrap_or_default(),
            ));
        }
    }
    Ok(())
}

async fn create_todo(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(input): Json<NewTodo>,
) -> ApiResult<Todo> {
    ensure_auth(&ctx, &headers)?;
    if input.title.trim().is_empty() {
        return Err(ApiError::bad_request("待办标题不能为空"));
    }
    let todo = ctx.db.create_todo(input)?;
    ctx.bus.publish(Event::new(
        "todo.created",
        serde_json::to_value(&todo).unwrap_or_default(),
    ));
    // 循环待办：新建根实例后补齐「下一期」（幂等；追溯创建过去日期的循环时尤其需要）
    publish_recurring_created(&ctx)?;
    Ok(ApiResp::ok(todo))
}

async fn list_todos(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<Todo>> {
    ensure_auth(&ctx, &headers)?;
    ctx.db.refresh_overdue().ok();
    Ok(ApiResp::ok(ctx.db.list_todos(
        q.get("category").map(|s| s.as_str()),
        q.get("status").map(|s| s.as_str()),
        q.get("priority").map(|s| s.as_str()),
        q.get("tag").map(|s| s.as_str()),
        q.get("q").map(|s| s.as_str()),
    )?))
}

async fn update_todo(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(patch): Json<TodoPatch>,
) -> ApiResult<Todo> {
    ensure_auth(&ctx, &headers)?;
    let todo = ctx
        .db
        .update_todo(id, patch)?
        .ok_or_else(|| ApiError::not_found("待办不存在"))?;
    ctx.bus.publish(Event::new(
        "todo.updated",
        serde_json::to_value(&todo).unwrap_or_default(),
    ));
    // 循环待办：改动（改期/改周期/恢复循环）后补齐「下一期」（幂等）
    publish_recurring_created(&ctx)?;
    Ok(ApiResp::ok(todo))
}

/// 删除待办。query scope=series 时删除整条循环链（根 + 已生成实例），
/// 普通待办或仅删单期用默认（单条）。删除后广播 todo.deleted（循环链广播全部受影响 id）。
async fn delete_todo(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let series = q.get("scope").map(|v| v == "series").unwrap_or(false);
    if series {
        let removed = ctx.db.delete_todo_series(id)?;
        ctx.bus.publish(Event::new(
            "todo.deleted",
            json!({ "id": id, "scope": "series", "removed": removed }),
        ));
        return Ok(ApiResp::ok(json!({ "deleted": removed > 0, "removed": removed })));
    }
    let ok = ctx.db.delete_todo(id)?;
    ctx.bus.publish(Event::new("todo.deleted", json!({ "id": id })));
    Ok(ApiResp::ok(json!({ "deleted": ok })))
}

async fn complete_todo(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Todo> {
    ensure_auth(&ctx, &headers)?;
    let todo = ctx
        .db
        .complete_todo(id, true)?
        .ok_or_else(|| ApiError::not_found("待办不存在"))?;
    ctx.bus.publish(Event::new(
        "todo.completed",
        serde_json::to_value(&todo).unwrap_or_default(),
    ));
    // 循环待办（weekly/monthly）：完成即补齐下一期（幂等），并广播新实例
    match ctx.db.ensure_recurring() {
        Ok(created) => {
            for t in created {
                ctx.bus.publish(Event::new(
                    "todo.created",
                    serde_json::to_value(&t).unwrap_or_default(),
                ));
            }
        }
        Err(e) => tracing::warn!("循环待办补期失败（不影响本次完成）：{e}"),
    }
    crate::achievements::check_all(&ctx.db, &ctx.bus)?;
    Ok(ApiResp::ok(todo))
}

async fn reopen_todo(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Todo> {
    ensure_auth(&ctx, &headers)?;
    let todo = ctx
        .db
        .complete_todo(id, false)?
        .ok_or_else(|| ApiError::not_found("待办不存在"))?;
    ctx.bus.publish(Event::new(
        "todo.updated",
        serde_json::to_value(&todo).unwrap_or_default(),
    ));
    Ok(ApiResp::ok(todo))
}

async fn schedule_for_date(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let date = q
        .get("date")
        .cloned()
        .unwrap_or_else(crate::db::today_string);
    let (schedules, todos) = ctx.db.schedule_for_date(&date)?;
    Ok(ApiResp::ok(json!({
        "date": date,
        "schedules": schedules,
        "todos": todos,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuggestReq {
    todo_id: i64,
}

/// 智能排期建议（FR-3.8）
async fn suggest_schedule(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<SuggestReq>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let Some(todo) = ctx.db.get_todo(req.todo_id)? else {
        return Err(ApiError::not_found("待办不存在"));
    };
    let cfg = ai::load_config(&ctx.db)?;
    let busy = ctx
        .db
        .list_todos(Some("日程"), Some("全部"), None, None, None)?;
    let busy_text = busy
        .iter()
        .take(20)
        .map(|t| format!("- {} {} {}", t.due_date, t.due_time.clone().unwrap_or_default(), t.title))
        .collect::<Vec<_>>()
        .join("\n");

    if !cfg.has_key && cfg.provider != "ollama" {
        // 降级：给出规则建议（下一个工作日且非周末）
        let today = chrono::Local::now().date_naive();
        let mut next = today + chrono::Duration::days(1);
        while next.weekday().num_days_from_monday() >= 5 {
            next += chrono::Duration::days(1);
        }
        return Ok(ApiResp::ok(json!({
            "isAi": false,
            "suggestion": format!("建议安排在 {}（就近的工作日）上午时段，优先处理逾期待办。", next.format("%Y-%m-%d")),
        })));
    }

    let prompt = format!(
        "用户的待办：{}\n截止：{}{}（优先级 {}{}）\n\n未来已有日程：\n{}\n\n今天是 {}。请用一句话给出排期建议（哪天、哪个时段、为什么），不超过 60 字。",
        ai::redact(&todo.title),
        todo.due_date,
        todo.due_time.clone().map(|t| format!(" {}", t)).unwrap_or_default(),
        todo.priority,
        if todo.overdue { "，已逾期" } else { "" },
        if busy_text.is_empty() { "（无）".into() } else { busy_text },
        crate::db::today_string()
    );
    let key = crate::secrets::load_api_key()?;
    let text = ai::chat_once(&cfg, vec![ChatMsg::user(prompt)], key)
        .await
        .map_err(|e| ApiError::ai(e.to_string()))?;
    Ok(ApiResp::ok(json!({ "isAi": true, "suggestion": text.trim() })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiTagReq {
    content: String,
}

/// AI 打标（v1.1.1）：从一段内容里提取 0~3 个标签，供速记/待办一键采纳。
/// AI 未配置时降级为本地规则：只回填「名字已出现在内容里的用户常用标签」。
async fn ai_tag(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<AiTagReq>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let content = req.content.trim().chars().take(2000).collect::<String>();
    if content.is_empty() {
        return Err(ApiError::bad_request("内容为空，无从打标"));
    }
    let existing = ctx.db.list_tags()?;
    let mut known: Vec<String> = existing.iter().map(|t| t.name.clone()).collect();
    // 自定义标签也是候选（AI 未配置时的本地降级全靠它命中：用户加的词就是他的心智词汇）
    if let Some(raw) = ctx.db.get_setting("custom_tags").ok().flatten() {
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(&raw) {
            for t in arr {
                let t = t.trim().to_string();
                if !t.is_empty() && !known.contains(&t) {
                    known.push(t);
                }
            }
        }
    }

    // 本地降级：已有标签（含自定义）的名字出现在内容里才算命中
    let local_tags: Vec<String> = known
        .iter()
        .filter(|t| !t.is_empty() && content.to_lowercase().contains(&t.to_lowercase()))
        .take(3)
        .cloned()
        .collect();

    let cfg = ai::load_config(&ctx.db)?;
    if !cfg.has_key && cfg.provider != "ollama" {
        return Ok(ApiResp::ok(json!({ "isAi": false, "tags": local_tags })));
    }

    let known_text = if known.is_empty() {
        "（暂无，可自拟）".to_string()
    } else {
        known.iter().take(30).cloned().collect::<Vec<_>>().join("、")
    };
    let prompt = format!(
        "从下面的内容中提取 0～3 个标签。优先从「已有标签」里选；确实没有合适的才新造（每个不超过 6 个字）。\
         只输出 JSON 字符串数组，例如 [\"工作\",\"AI\"]，不要输出任何解释。\n\n已有标签：{}\n\n内容：{}",
        known_text,
        ai::redact(&content)
    );
    let key = crate::secrets::load_api_key()?;
    match ai::chat_once(&cfg, vec![ChatMsg::user(prompt)], key).await {
        Ok(text) => {
            let tags = parse_tag_array(&text);
            if tags.is_empty() {
                // 模型没按格式给：退回本地规则，用户侧不至于空手而归
                Ok(ApiResp::ok(json!({ "isAi": false, "tags": local_tags })))
            } else {
                Ok(ApiResp::ok(json!({ "isAi": true, "tags": tags })))
            }
        }
        Err(e) => Err(ApiError::ai(e.to_string())),
    }
}

/// 从模型回复里抠出 JSON 字符串数组（容忍 ```json 包裹与前后废话），
/// 清洗：去空白、长度 ≤ 12 字、去重、最多 3 个。
fn parse_tag_array(text: &str) -> Vec<String> {
    let raw = match (text.find('['), text.rfind(']')) {
        (Some(s), Some(e)) if e > s => &text[s..=e],
        _ => return Vec::new(),
    };
    let Ok(arr) = serde_json::from_str::<Vec<String>>(raw) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for t in arr {
        let t = t.trim().trim_start_matches('#').trim().to_string();
        if t.is_empty() || t.chars().count() > 12 || out.contains(&t) {
            continue;
        }
        out.push(t);
        if out.len() >= 3 {
            break;
        }
    }
    out
}

// ───────────────────────── 设置 ─────────────────────────

async fn all_settings(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<Vec<Setting>> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(ctx.db.all_settings()?))
}

async fn get_setting(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(json!({ "key": key, "value": ctx.db.get_setting(&key)? })))
}

#[derive(Deserialize)]
struct SettingsUpdate {
    values: HashMap<String, String>,
}

async fn update_settings(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<SettingsUpdate>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    for (k, v) in &req.values {
        ctx.db.set_setting(k, v)?;
    }
    ctx.bus.publish(Event::new(
        "settings.updated",
        json!({ "keys": req.values.keys().cloned().collect::<Vec<_>>() }),
    ));
    Ok(ApiResp::ok(json!({ "updated": req.values.len() })))
}

// ───────────────────────── 模板 ─────────────────────────

async fn all_templates(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let stored: HashMap<String, String> = ctx.db.all_templates()?.into_iter().collect();
    let lang = ctx.lang();
    let builtin = json!({
        "daily": ai::default_template("daily", lang),
        "weekly": ai::default_template("weekly", lang),
        "monthly": ai::default_template("monthly", lang),
        "brief": ai::default_template("brief", lang),
        "goodnight": ai::default_template("goodnight", lang),
        "review": ai::default_template("review", lang),
        "qa": ai::default_template("qa", lang),
    });
    let mut merged = builtin.clone();
    for (k, v) in stored {
        merged[k] = json!(v);
    }
    Ok(ApiResp::ok(json!({ "templates": merged, "builtin": builtin })))
}

async fn get_template(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(ttype): Path<String>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let stored = ctx.db.get_template(&ttype)?;
    let builtin = ai::default_template(ttype.as_str(), ctx.lang());
    Ok(ApiResp::ok(json!({
        "type": ttype,
        "content": stored.clone().unwrap_or_else(|| builtin.to_string()),
        "builtin": builtin,
        "customized": stored.is_some(),
    })))
}

#[derive(Deserialize)]
struct TemplateBody {
    content: String,
}

async fn set_template(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(ttype): Path<String>,
    Json(body): Json<TemplateBody>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    ctx.db.set_template(&ttype, &body.content)?;
    Ok(ApiResp::ok(json!({ "ok": true })))
}

// ───────────────────────── AI ─────────────────────────

async fn ai_presets(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(json!(ai::presets())))
}

async fn ai_config(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<AiConfig> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(ai::load_config(&ctx.db)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiConfigBody {
    provider: String,
    base_url: String,
    model: String,
    temperature: f32,
    max_tokens: i64,
    /// 协议模式：auto（默认）/ openai / anthropic
    protocol_mode: Option<String>,
    /// 仅当用户输入新 Key 时传
    api_key: Option<String>,
}

async fn ai_save_config(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(body): Json<AiConfigBody>,
) -> ApiResult<AiConfig> {
    ensure_auth(&ctx, &headers)?;
    ai::save_config(
        &ctx.db,
        &body.provider,
        &body.base_url,
        &body.model,
        body.temperature,
        body.max_tokens,
        body.protocol_mode.as_deref(),
    )?;
    if let Some(key) = body.api_key {
        let key = key.trim();
        if !key.is_empty() {
            crate::secrets::save_api_key(key)
                .map_err(|e| ApiError::internal(format!("保存 API Key 失败：{e}")))?;
        }
    }
    ctx.bus.publish(Event::new("settings.updated", json!({ "keys": ["ai"] })));
    Ok(ApiResp::ok(ai::load_config(&ctx.db)?))
}

/// 测试连接：无论成功失败都返回 200 + 结构化结论
///
/// 为什么不再返回 HTTP 错误：界面的职责不是显示报错，而是告诉用户**下一步该做什么**
/// （401 去换 Key、404 去补 /v1、429 稍后再试…）。原来的实现把上游状态码塞进一句中文
/// 报错里，前端只能原样显示，用户看不懂。这里把「类别 + 上游状态码 + 原文」结构化返回，
/// 由 ai::classify 统一判定（判定逻辑同时被单测覆盖）。
async fn ai_test(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let cfg = ai::load_config(&ctx.db)?;
    let key = crate::secrets::load_api_key()?;
    let started = std::time::Instant::now();
    let verdict = match ai::chat_once(&cfg, vec![ChatMsg::user("请只回复两个字：你好")], key).await {
        Ok(reply) => ai::TestVerdict::success(&cfg.model, started.elapsed().as_millis() as i64, &reply),
        Err(e) => ai::TestVerdict::failure(&e, &cfg.model, started.elapsed().as_millis() as i64),
    };
    Ok(ApiResp::ok(
        serde_json::to_value(verdict).map_err(|e| ApiError::internal(e.to_string()))?,
    ))
}

/// 探测本机 Ollama：让用户一键确认「本地模型能不能用」，并列出已装模型
async fn ai_ollama_probe(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(
        serde_json::to_value(ai::probe_ollama().await).map_err(|e| ApiError::internal(e.to_string()))?,
    ))
}

#[derive(Deserialize)]
struct OpenUrlReq {
    url: String,
}

/// 用系统默认浏览器打开外链（设置页「去申请 Key」用）
///
/// 只在本地模式开放：桌面端需要跳出 WebView 打开厂商页面，而局域网/服务器模式
/// 是别人在远程访问，绝不该能借这台机器调起浏览器进程。
// ───────────────────────── 今日热点 ─────────────────────────

/// 全部在用标签及使用次数（标签选择器数据源；前端再并上默认标签与用户自定义词）
async fn list_tags(State(ctx): State<Arc<AppContext>>, headers: HeaderMap) -> ApiResult<Vec<TagStat>> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(ctx.db.list_tags()?))
}

/// 可用栏目清单（由 news::CHANNELS 注册表自动生成，设置页多选用）
async fn news_channels(State(ctx): State<Arc<AppContext>>, headers: HeaderMap) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let channels: Vec<serde_json::Value> = crate::news::CHANNELS
        .iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    let focus: Vec<serde_json::Value> = crate::news::FOCUS_TOPICS
        .iter()
        .map(|(id, name, _)| json!({ "id": id, "name": name }))
        .collect();
    Ok(ApiResp::ok(json!({ "channels": channels, "focus": focus })))
}

/// 抓取热点新闻。query：
/// - refresh=1  跳过缓存强制实抓
/// - channels   逗号分隔栏目 id（缺省读设置 news_channels，默认 weibo）
/// - limit      条数上限（缺省读设置 news_limit，默认 10）
/// - focus      逗号分隔重点关注行业 id（缺省读设置 news_focus）
/// - kw         逗号分隔自定义关注关键词（缺省读设置 news_focus_keywords）
///   focus（行业）非空：抓 200 条热搜大池子按行业关键词过滤；
///   kw（自定义关键词）非空：直接搜索互联网（必应中国），按相关度降序，条目来源标识为该关键词
async fn news_hot(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<crate::news::HotNewsResult> {
    ensure_auth(&ctx, &headers)?;
    let refresh = q.get("refresh").map(|v| v == "1").unwrap_or(false);
    let limit = q
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .or_else(|| ctx.db.get_setting("news_limit").ok().flatten().and_then(|v| v.parse().ok()))
        .unwrap_or(10)
        .clamp(1, 200);
    let channels: Vec<String> = match q.get("channels") {
        Some(raw) if !raw.is_empty() => raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        _ => ctx
            .db
            .get_setting("news_channels")
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_else(|| vec!["weibo".to_string()]),
    };
    let channels: Vec<String> = if channels.is_empty() { vec!["weibo".to_string()] } else { channels };

    // 重点关注：请求参数优先，其次读用户设置
    let parse_list = |raw: Option<&String>| -> Vec<String> {
        raw.map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let focus_ids: Vec<String> = match q.get("focus") {
        Some(raw) if !raw.trim().is_empty() => parse_list(Some(raw)),
        _ => ctx
            .db
            .get_setting("news_focus")
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_default(),
    }
    .into_iter()
    .filter(|id| crate::news::valid_focus(id))
    .collect();
    let custom_keywords: Vec<String> = match q.get("kw") {
        Some(raw) if !raw.trim().is_empty() => parse_list(Some(raw)),
        _ => ctx
            .db
            .get_setting("news_focus_keywords")
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_default(),
    };
    let focusing = !focus_ids.is_empty() || !custom_keywords.is_empty();

    // 重点关注模式下缓存不适用（过滤后的条数不能反映池子大小），始终走实抓逻辑
    if !refresh && !focusing {
        if let Some((cached, at)) = crate::news::load_cache(&ctx.db) {
            if cached.items.len() >= limit {
                if let Ok(ts) = chrono::NaiveDateTime::parse_from_str(&at, "%Y-%m-%d %H:%M:%S") {
                    let age = chrono::Local::now().naive_local() - ts;
                    if age.num_minutes() < 30 {
                        let mut r = cached;
                        r.source = "cache".into();
                        r.stale = false; // 30 分钟内的缓存命中属正常路径，不提示「非实时」
                        r.items.truncate(limit);
                        r.errors.clear();
                        return Ok(ApiResp::ok(r));
                    }
                }
            }
        }
    }
    // 重点关注模式：行业走热搜大池子过滤；自定义关键词走互联网搜索（热搜榜覆盖不了小众词）。
    // 结果连同关注参数签名一起缓存：跨页面切回（非 refresh）时直接展示上次记录，
    // 不重复打源站；实抓失败/无命中时回退上次缓存记录（stale 标记），绝不让面板全空。
    if focusing {
        let sig = format!("{}|{}", focus_ids.join(","), custom_keywords.join(","));
        if !refresh {
            if let Some((cached, at)) = crate::news::load_cache_key(&ctx.db, crate::news::FOCUS_CACHE_KEY) {
                if cached.params == sig {
                    if let Ok(ts) = chrono::NaiveDateTime::parse_from_str(&at, "%Y-%m-%d %H:%M:%S") {
                        let age = chrono::Local::now().naive_local() - ts;
                        if age.num_minutes() < 30 {
                            let mut r = cached;
                            r.source = "cache".into();
                            r.stale = false;
                            r.errors.clear();
                            r.items.truncate(limit);
                            return Ok(ApiResp::ok(r));
                        }
                    }
                }
            }
        }
        let fetched_at = crate::db::now_string();
        let mut items: Vec<crate::news::NewsItem> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        if !focus_ids.is_empty() {
            let (groups, errs) = crate::news::fetch_channels(&channels, 200).await;
            errors.extend(errs);
            let pool = crate::news::merge_items(groups, 200);
            items.extend(crate::news::filter_focus(pool, &focus_ids, &[]));
        }
        if !custom_keywords.is_empty() {
            items.extend(crate::news::search_keywords(&custom_keywords, 8).await);
        }
        items.truncate(limit);
        if !items.is_empty() {
            // 成功：连同参数签名写入专属缓存，供跨页面切回时展示
            let result = crate::news::HotNewsResult {
                items,
                source: "live".into(),
                fetched_at,
                errors,
                stale: false,
                params: sig,
            };
            crate::news::save_cache_key(&ctx.db, crate::news::FOCUS_CACHE_KEY, &result);
            return Ok(ApiResp::ok(result));
        }
        // 实抓失败/无命中：回退上次缓存记录（任何参数的），绝不让面板全空
        if let Some((mut cached, at)) = crate::news::load_cache_key(&ctx.db, crate::news::FOCUS_CACHE_KEY) {
            cached.source = "cache".into();
            cached.stale = true;
            cached.fetched_at = at;
            cached.errors = vec!["本次抓取失败，正在展示最近一次成功的数据".to_string()];
            cached.items.truncate(limit);
            return Ok(ApiResp::ok(cached));
        }
        // 连缓存都没有（首次使用即失败）：空结果
        return Ok(ApiResp::ok(crate::news::HotNewsResult {
            items: Vec::new(),
            source: "none".into(),
            fetched_at,
            errors,
            stale: false,
            params: sig,
        }));
    }

    let result = crate::news::fetch_hot_news(&ctx.db, &channels, limit).await;
    Ok(ApiResp::ok(result))
}

async fn system_open_url(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<OpenUrlReq>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    if ctx.cfg.mode != crate::RunMode::Local {
        return Err(ApiError::bad_request(
            "仅本地模式支持直接打开链接，请手动复制链接到浏览器",
        ));
    }
    let url = crate::system::validate_external_url(&req.url).map_err(ApiError::bad_request)?;
    crate::system::open_in_browser(&url)
        .map_err(|e| ApiError::internal(format!("调起浏览器失败：{e}")))?;
    Ok(ApiResp::ok(json!({ "opened": true, "url": url })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportReq {
    r#type: String,
    date: Option<String>,
}

/// 生成报告（流式 SSE）
async fn ai_report(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<ReportReq>,
) -> Result<SseStream, ApiError> {
    ensure_auth(&ctx, &headers)?;
    let rtype = req.r#type.clone();
    let date = req.date.unwrap_or_else(crate::db::today_string);
    let ctx2 = ctx.clone();
    let stream = ai_stream_from(ctx2, move |ctx| {
        let rtype = rtype.clone();
        let date = date.clone();
        async move { build_report_messages(&ctx, &rtype, &date).await }
    })
    .await?;
    Ok(stream)
}

/// 构造报告消息（含 AI 降级判断）
async fn build_report_messages(
    ctx: &Arc<AppContext>,
    rtype: &str,
    date: &str,
) -> Result<StreamPlan, String> {
    let cfg = ai::load_config(&ctx.db).map_err(|e| e.to_string())?;
    if !cfg.has_key && cfg.provider != "ollama" {
        // 降级：本地模板拼装（落库由 ai_stream_from 统一处理）
        let lang = ctx.lang();
        let content = ai::fallback_report_lang(&ctx.db, rtype, date, lang).map_err(|e| e.to_string())?;
        let period = if rtype == "daily" { date.to_string() } else { ai::period_range(rtype, date).2 };
        return Ok(StreamPlan::degraded(content, Some((rtype.to_string(), period))));
    }

    let (from, to, label) = ai::period_range(rtype, date);
    let (tpl_key, vars): (&str, Vec<(&str, String)>) = match rtype {
        "daily" => {
            let nodes = ctx.db.list_nodes_by_date(date).map_err(|e| e.to_string())?;
            let (_, todos) = ctx.db.schedule_for_date(date).map_err(|e| e.to_string())?;
            let stats = ctx.db.daily_stats(date).map_err(|e| e.to_string())?;
            (
                "daily",
                vec![
                    ("date", date.to_string()),
                    ("nodes", ai::format_nodes(&nodes)),
                    ("todos", ai::format_todos(&todos)),
                    (
                        "progress",
                        ai::format_progress(
                            stats.node_count,
                            stats.daily_goal,
                            stats.goal_enabled,
                            stats.total_todos,
                            stats.done_todos,
                        ),
                    ),
                ],
            )
        }
        _ => {
            let nodes = ctx.db.list_nodes_range(&from, &to).map_err(|e| e.to_string())?;
            let all = ctx
                .db
                .list_todos(Some("全部"), Some("全部"), None, None, None)
                .map_err(|e| e.to_string())?;
            let period_todos: Vec<Todo> = all
                .into_iter()
                .filter(|t| t.due_date >= from && t.due_date <= to)
                .collect();
            let stats = ctx.db.period_stats(&from, &to).map_err(|e| e.to_string())?;
            (
                if rtype == "weekly" { "weekly" } else { "monthly" },
                vec![
                    ("period", label.clone()),
                    ("from", from.clone()),
                    ("to", to.clone()),
                    ("nodes", ai::format_nodes_by_day(&nodes)),
                    ("todos", ai::format_todos(&period_todos)),
                    (
                        "progress",
                        format!(
                            "记录 {} 条，覆盖 {} / {} 天；待办完成 {}/{}",
                            stats.node_count,
                            stats.days_with_records,
                            stats.total_days,
                            stats.done_todos,
                            stats.total_todos
                        ),
                    ),
                ],
            )
        }
    };

    let tpl = ctx
        .db
        .get_template(tpl_key)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| ai::default_template(tpl_key, ctx.lang()));
    let prompt = ai::render_template(&tpl, &vars);
    let period = if rtype == "daily" { date.to_string() } else { label };
    Ok(StreamPlan::model(vec![ChatMsg::user(prompt)]).with_report(rtype, &period))
}

/// 统一的 SSE 流类型（装箱以统一分支类型）
type SseStream = Sse<
    KeepAliveStream<Pin<Box<dyn Stream<Item = Result<SseEvent, std::convert::Infallible>> + Send>>>,
>;

fn boxed_sse<S>(s: S) -> SseStream
where
    S: Stream<Item = Result<SseEvent, std::convert::Infallible>> + Send + 'static,
{
    let pinned: Pin<Box<dyn Stream<Item = Result<SseEvent, std::convert::Infallible>> + Send>> =
        Box::pin(s);
    Sse::new(pinned).keep_alive(KeepAlive::default())
}

/// 流式任务计划：由各 handler 构造，显式声明落库目标与会话
pub struct StreamPlan {
    /// 发给模型的消息（为空表示走本地降级，直接输出 content）
    pub messages: Vec<ChatMsg>,
    /// 降级模式下直接输出的内容
    pub content: String,
    /// 报告落库目标 (类型, 周期)：daily/weekly/monthly/brief/goodnight/review
    pub save_as: Option<(String, String)>,
    /// 问答会话：需要把助手回复写入 chat_messages 时提供
    pub chat_session: Option<String>,
}

impl StreamPlan {
    /// 走模型：只有消息，无落库
    pub fn model(messages: Vec<ChatMsg>) -> Self {
        Self { messages, content: String::new(), save_as: None, chat_session: None }
    }
    /// 本地降级：直接输出内容，并按需落库
    pub fn degraded(content: String, save_as: Option<(String, String)>) -> Self {
        Self { messages: vec![], content, save_as, chat_session: None }
    }
    pub fn with_report(mut self, rtype: &str, period: &str) -> Self {
        self.save_as = Some((rtype.to_string(), period.to_string()));
        self
    }
    pub fn with_chat(mut self, session: &str) -> Self {
        self.chat_session = Some(session.to_string());
        self
    }
}

/// 通用 SSE 流式封装：消息构造 → 上游流式 → 转发 → 落库（统一在此处理持久化，避免路径分叉）
async fn ai_stream_from<F, Fut>(ctx: Arc<AppContext>, build: F) -> Result<SseStream, ApiError>
where
    F: FnOnce(Arc<AppContext>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<StreamPlan, String>> + Send,
{
    let plan = build(ctx.clone()).await.map_err(ApiError::ai)?;

    // ── 降级路径：内容已由 handler 组装，这里统一落库 + 评估成就 ──
    if plan.messages.is_empty() {
        let content = plan.content.clone();
        if let Some((rtype, period)) = &plan.save_as {
            if let Err(e) = ctx.db.save_report(rtype, period, &content, false) {
                tracing::warn!("降级报告落库失败: {e}");
            }
        }
        if let Some(session) = &plan.chat_session {
            if let Err(e) = ctx.db.append_chat(session, "assistant", &content) {
                tracing::warn!("降级问答落库失败: {e}");
            }
        }
        let _ = crate::achievements::check_all(&ctx.db, &ctx.bus);

        let chunks: Vec<String> = content
            .chars()
            .collect::<Vec<_>>()
            .chunks(24)
            .map(|c| c.iter().collect())
            .collect();
        let s = futures_util::stream::iter(chunks.into_iter().map(move |c| {
            Ok(SseEvent::default()
                .event("delta")
                .data(json!({ "delta": c, "degraded": true }).to_string()))
        }));
        return Ok(boxed_sse(s));
    }

    // ── 模型路径 ──
    let cfg = ai::load_config(&ctx.db).map_err(|e| ApiError::internal(e.to_string()))?;
    let key = crate::secrets::load_api_key().unwrap_or(None);
    let upstream = ai::chat_stream(&cfg, plan.messages.clone(), key)
        .await
        .map_err(|e| ApiError::ai(e.to_string()))?;

    let db = ctx.db.clone();
    let bus = ctx.bus.clone();
    let save_as = plan.save_as.clone();
    let chat_session = plan.chat_session.clone();
    let s = async_stream::stream! {
        futures_util::pin_mut!(upstream);
        let mut acc = String::new();
        while let Some(item) = upstream.next().await {
            match item {
                Ok(delta) => {
                    acc.push_str(&delta);
                    yield Ok(SseEvent::default().event("delta").data(json!({"delta": delta}).to_string()));
                }
                Err(e) => {
                    yield Ok(SseEvent::default().event("error").data(json!({"message": e.to_string()}).to_string()));
                    return;
                }
            }
        }
        // 落库：报告归档 / 问答助手回复（此前 AI 路径遗漏，导致历史缺失）
        if !acc.is_empty() {
            if let Some((rtype, period)) = &save_as {
                if let Err(e) = db.save_report(rtype, period, &acc, true) {
                    tracing::warn!("报告落库失败: {e}");
                }
                bus.publish(Event::new("report.saved", json!({"type": rtype, "period": period})));
            }
            if let Some(session) = &chat_session {
                if let Err(e) = db.append_chat(session, "assistant", &acc) {
                    tracing::warn!("问答落库失败: {e}");
                }
            }
            let _ = crate::achievements::check_all(&db, &bus);
        }
        yield Ok(SseEvent::default().event("done").data(json!({"ok": true}).to_string()));
    };

    Ok(boxed_sse(s))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BriefReq {
    date: Option<String>,
}

/// 晨间简报（FR-5.5）
async fn ai_brief(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<BriefReq>,
) -> Result<SseStream, ApiError> {
    ensure_auth(&ctx, &headers)?;
    let date = req.date.unwrap_or_else(crate::db::today_string);
    let stream = ai_stream_from(ctx.clone(), move |ctx| {
        let date = date.clone();
        async move {
            let cfg = ai::load_config(&ctx.db).map_err(|e| e.to_string())?;
            let yesterday = (chrono::Local::now().date_naive() - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            let all = ctx
                .db
                .list_todos(Some("全部"), Some("全部"), None, None, None)
                .map_err(|e| e.to_string())?;
            // 昨日遗留：截止日在昨天及以前、或已逾期
            let pending: Vec<Todo> = all
                .iter()
                .filter(|t| {
                    (t.due_date <= yesterday && t.status != "已完成")
                        || t.status == "已逾期"
                })
                .cloned()
                .collect();
            let today_todos: Vec<Todo> = all
                .iter()
                .filter(|t| t.due_date == date)
                .cloned()
                .collect();
            // 今天已录入的记录节点：简报要「汇总待办 + 今日记录」再润色，
            // 缺了它就会出现「刚记完却不在简报里」的错觉（用户实际反馈过）
            let today_nodes = ctx
                .db
                .list_nodes_by_date(&date)
                .map_err(|e| e.to_string())?;

            if !cfg.has_key && cfg.provider != "ollama" {
                // 降级：规则拼装简报（落库由 ai_stream_from 统一处理）
                let mut md =
                    format!("# ☀️ 今日简报\n\n> 本地模板生成（未配置 AI 模型）\n\n");
                // 今天已记录的内容：使用者希望简报能反映当日实际录入，
                // 因此降级模板也必须包含（否则「录入了却看不到」）
                if !today_nodes.is_empty() {
                    md.push_str("## 今日进展\n\n");
                    md.push_str(&ai::format_nodes(&today_nodes));
                    md.push_str("\n\n");
                }
                md.push_str("## 昨日遗留\n\n");
                if pending.is_empty() {
                    md.push_str("- 无，干得漂亮 ✨\n");
                } else {
                    for t in pending.iter().take(5) {
                        md.push_str(&format!("- {}（截止 {}）\n", t.title, t.due_date));
                    }
                }
                md.push_str("\n## 今日安排\n\n");
                if today_todos.is_empty() {
                    md.push_str("- 暂无待办\n");
                } else {
                    for t in &today_todos {
                        md.push_str(&format!(
                            "- {}{} {}\n",
                            t.due_time
                                .clone()
                                .map(|x| format!("{x} "))
                                .unwrap_or_default(),
                            t.title,
                            if t.priority == "高" { "（高优先级）" } else { "" }
                        ));
                    }
                }
                md.push_str("\n## 建议\n\n1. 先处理逾期待办\n2. 再按优先级推进今日重点\n");
                return Ok(StreamPlan::degraded(
                    md,
                    Some(("brief".into(), date.clone())),
                ));
            }

            let tpl = ctx
                .db
                .get_template("brief")
                .map_err(|e| e.to_string())?
                .unwrap_or_else(|| ai::DEFAULT_BRIEF.to_string());
            let prompt = ai::render_template(
                &tpl,
                &[
                    ("date", date.clone()),
                    ("nodes", ai::format_nodes(&today_nodes)),
                    ("todos", ai::format_todos(&pending)),
                    ("today", ai::format_todos(&today_todos)),
                ],
            );
            Ok(StreamPlan::model(vec![ChatMsg::user(prompt)]).with_report("brief", &date))
        }
    })

    .await?;
    Ok(stream)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoodnightReq {
    date: Option<String>,
}

/// 晚安总结（FR-5.9）
async fn ai_goodnight(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<GoodnightReq>,
) -> Result<SseStream, ApiError> {
    ensure_auth(&ctx, &headers)?;
    let date = req.date.unwrap_or_else(crate::db::today_string);
    let stream = ai_stream_from(ctx.clone(), move |ctx| {
        let date = date.clone();
        async move {
            let cfg = ai::load_config(&ctx.db).map_err(|e| e.to_string())?;
            let nodes = ctx.db.list_nodes_by_date(&date).map_err(|e| e.to_string())?;
            let (_, todos) = ctx.db.schedule_for_date(&date).map_err(|e| e.to_string())?;

            if !cfg.has_key && cfg.provider != "ollama" {
                let done: Vec<&Todo> = todos.iter().filter(|t| t.status == "已完成").collect();
                let undone: Vec<&Todo> = todos.iter().filter(|t| t.status != "已完成").collect();
                let mut md =
                    format!("# 🌙 晚安总结\n\n> 本地模板生成（未配置 AI 模型）\n\n## 今天完成了\n\n");
                if nodes.is_empty() {
                    md.push_str("- （今日无记录）\n");
                } else {
                    for n in nodes.iter().take(8) {
                        let t = n.created_at.get(11..16).unwrap_or("--:--");
                        md.push_str(&format!("- {t} {}\n", n.content));
                    }
                }
                md.push_str("\n## 待办情况\n\n");
                md.push_str(&format!(
                    "- 已完成 {} 项\n- 未完成 {} 项\n",
                    done.len(),
                    undone.len()
                ));
                if !undone.is_empty() {
                    md.push_str("\n## 明日建议\n\n");
                    for t in undone.iter().take(3) {
                        md.push_str(&format!("- {}\n", t.title));
                    }
                }
                return Ok(StreamPlan::degraded(
                    md,
                    Some(("goodnight".into(), date.clone())),
                ));
            }

            let tpl = ctx
                .db
                .get_template("goodnight")
                .map_err(|e| e.to_string())?
                .unwrap_or_else(|| ai::DEFAULT_GOODNIGHT.to_string());
            let prompt = ai::render_template(
                &tpl,
                &[
                    ("date", date.clone()),
                    ("nodes", ai::format_nodes(&nodes)),
                    ("todos", ai::format_todos(&todos)),
                ],
            );
            Ok(StreamPlan::model(vec![ChatMsg::user(prompt)]).with_report("goodnight", &date))
        }
    })
    .await?;
    Ok(stream)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewReq {
    date: Option<String>,
}

/// 周度智能复盘（FR-5.10）
async fn ai_review(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<ReviewReq>,
) -> Result<SseStream, ApiError> {
    ensure_auth(&ctx, &headers)?;
    let date = req.date.unwrap_or_else(crate::db::today_string);
    let stream = ai_stream_from(ctx.clone(), move |ctx| {
        let date = date.clone();
        async move {
            let (from, to, label) = ai::period_range("weekly", &date);
            let stats = ctx.db.period_stats(&from, &to).map_err(|e| e.to_string())?;
            let nodes = ctx.db.list_nodes_range(&from, &to).map_err(|e| e.to_string())?;
            let all = ctx
                .db
                .list_todos(Some("全部"), Some("全部"), None, None, None)
                .map_err(|e| e.to_string())?;
            let week_todos: Vec<Todo> = all
                .into_iter()
                .filter(|t| t.due_date >= from && t.due_date <= to)
                .collect();

            let cfg = ai::load_config(&ctx.db).map_err(|e| e.to_string())?;
            let days_text = stats
                .days
                .iter()
                .map(|d| {
                    format!(
                        "{}: 记录 {} 条, 待办 {}/{}",
                        d.date, d.node_count, d.done_todos, d.total_todos
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            if !cfg.has_key && cfg.provider != "ollama" {
                // 降级：本地统计洞察
                let busiest = stats
                    .days
                    .iter()
                    .max_by_key(|d| d.node_count)
                    .map(|d| d.date.clone())
                    .unwrap_or_default();
                let worst = stats
                    .days
                    .iter()
                    .filter(|d| d.total_todos > 0)
                    .min_by_key(|d| d.done_todos * 100 / d.total_todos.max(1))
                    .map(|d| (d.date.clone(), d.total_todos, d.done_todos));
                let mut md = format!("# 📊 {label} 复盘\n\n> 本地统计生成（未配置 AI 模型）\n\n");
                md.push_str(&format!(
                    "- 本周记录 **{}** 条，覆盖 **{}/{}** 天\n- 待办完成 **{}/{}**\n- 记录最多的一天：**{}**\n",
                    stats.node_count,
                    stats.days_with_records,
                    stats.total_days,
                    stats.done_todos,
                    stats.total_todos,
                    busiest
                ));
                if let Some((d, total, done)) = worst {
                    md.push_str(&format!("- 完成率最低：**{d}**（{done}/{total}）\n"));
                }
                md.push_str(
                    "\n## 下周建议\n\n1. 保持每日至少一条记录\n2. 关注完成率偏低的日子，提前拆分任务\n",
                );
                return Ok(StreamPlan::degraded(
                    md,
                    Some(("review".into(), label.clone())),
                ));
            }

            let tpl = ctx
                .db
                .get_template("review")
                .map_err(|e| e.to_string())?
                .unwrap_or_else(|| ai::DEFAULT_REVIEW.to_string());
            let prompt = ai::render_template(
                &tpl,
                &[
                    ("period", label.clone()),
                    ("days", days_text),
                    ("todos", ai::format_todos(&week_todos)),
                    (
                        "progress",
                        format!(
                            "记录 {} 条，覆盖 {}/{} 天；待办完成 {}/{}；本周记录节点数 {}",
                            stats.node_count,
                            stats.days_with_records,
                            stats.total_days,
                            stats.done_todos,
                            stats.total_todos,
                            nodes.len()
                        ),
                    ),
                ],
            );
            Ok(StreamPlan::model(vec![ChatMsg::user(prompt)]).with_report("review", &label))
        }
    })
    .await?;
    Ok(stream)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatReq {
    question: String,
    session_id: Option<String>,
}

/// 智伴问答（FR-5.11）：本地检索组装上下文 → 模型回答，助手回复统一落库
async fn ai_chat(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<ChatReq>,
) -> Result<SseStream, ApiError> {
    ensure_auth(&ctx, &headers)?;
    let session = req.session_id.unwrap_or_else(|| "default".into());
    let question = req.question.clone();
    if question.trim().is_empty() {
        return Err(ApiError::bad_request("问题不能为空"));
    }
    ctx.db.append_chat(&session, "user", &question)?;

    let stream = ai_stream_from(ctx.clone(), move |ctx| {
        let question = question.clone();
        let session = session.clone();
        async move {
            let today = chrono::Local::now().date_naive();
            let scope = ai::qa_scope(&question, today);
            let (from, to) = (scope.from.clone(), scope.to.clone());
            let mut context = String::new();

            // 明确日期 → 单日检索；否则按「本周/本月/昨天/最近 7 天」范围检索
            if let Some(ref day) = scope.single_day {
                let day = day.clone();
                let nodes = ctx.db.list_nodes_by_date(&day).map_err(|e| e.to_string())?;
                let (_, todos) = ctx.db.schedule_for_date(&day).map_err(|e| e.to_string())?;
                context.push_str(&format!("## {day} 的记录\n{}\n", ai::format_nodes(&nodes)));
                context.push_str(&format!("## {day} 的待办\n{}\n", ai::format_todos(&todos)));
            } else {
                let nodes = ctx.db.list_nodes_range(&from, &to).map_err(|e| e.to_string())?;
                context.push_str(&format!(
                    "## {from} ~ {to} 的记录\n{}\n",
                    ai::format_nodes_by_day(&nodes)
                ));
                let all = ctx
                    .db
                    .list_todos(Some("全部"), Some("全部"), None, None, None)
                    .map_err(|e| e.to_string())?;
                let range_todos: Vec<Todo> = all
                    .into_iter()
                    .filter(|t| t.due_date >= from && t.due_date <= to)
                    .collect();
                context.push_str(&format!(
                    "## {from} ~ {to} 的待办\n{}\n",
                    ai::format_todos(&range_todos)
                ));
            }

            let cfg = ai::load_config(&ctx.db).map_err(|e| e.to_string())?;
            if !cfg.has_key && cfg.provider != "ollama" {
                // 降级：直接返回本地检索结果（助手回复由 ai_stream_from 落库）
                let reply = format!("（未配置 AI 模型，以下是本地检索结果）\n\n{context}");
                return Ok(StreamPlan::degraded(reply, None).with_chat(&session));
            }

            let tpl = ctx
                .db
                .get_template("qa")
                .map_err(|e| e.to_string())?
                .unwrap_or_else(|| ai::DEFAULT_QA.to_string());
            let prompt = ai::render_template(
                &tpl,
                &[("context", context), ("question", question.clone())],
            );
            Ok(StreamPlan::model(vec![ChatMsg::user(format!(
                "{prompt}\n\n# 用户问题\n{question}"
            ))])
            .with_chat(&session))
        }
    })
    .await?;
    Ok(stream)
}

/// 智能排期（别名，与前端路由对齐）
async fn ai_replan(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(req): Json<SuggestReq>,
) -> ApiResult<serde_json::Value> {
    suggest_schedule(State(ctx), headers, Json(req)).await
}

// ───────────────────────── 报告 / 聊天 / 成就 / 数据 ─────────────────────────

/// 安装信息：首见证据（老用户识别埋点）与版本号
///
/// 第二期客户端注册/登录时会把这里的证据一并上报，服务端据此判定老用户并赠送 12 个月 Pro。
/// 一期仅需记录与可查（便于排查与用户申诉）。
async fn install_info(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let v = crate::firstseen::current(
        &ctx.db,
        &ctx.cfg.data_dir,
        &ctx.jwt_secret,
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(ApiResp::ok(serde_json::json!({
        "firstSeenAt": v.at,
        "firstSeenVersion": v.version,
        "installId": v.install_id,
        "source": v.source,
        "signatureValid": v.signature_valid,
        "appVersion": env!("CARGO_PKG_VERSION"),
    })))
}

async fn list_reports(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<Report>> {
    ensure_auth(&ctx, &headers)?;
    let t = q.get("type").map(|s| s.as_str());
    let limit: i64 = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100);
    Ok(ApiResp::ok(ctx.db.list_reports(t, limit)?))
}

async fn delete_report(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(json!({ "deleted": ctx.db.delete_report(id)? })))
}

async fn list_chat(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<ChatMessage>> {
    ensure_auth(&ctx, &headers)?;
    let session = q.get("sessionId").cloned().unwrap_or_else(|| "default".into());
    Ok(ApiResp::ok(ctx.db.list_chat(&session, 200)?))
}

async fn clear_chat(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    let session = q.get("sessionId").cloned().unwrap_or_else(|| "default".into());
    ctx.db.clear_chat(&session)?;
    Ok(ApiResp::ok(json!({ "ok": true })))
}

/// 成就目录（含解锁状态与友好名称，FR-6.2）
async fn list_achievements(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<Vec<crate::achievements::AchievementDef>> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(crate::achievements::catalog(&ctx.db)?))
}

/// 主动触发一次成就评估（前端查看成就时调用，保证展示最新状态）
async fn check_achievements_api(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<Vec<crate::achievements::AchievementDef>> {
    ensure_auth(&ctx, &headers)?;
    crate::achievements::check_all(&ctx.db, &ctx.bus)?;
    Ok(ApiResp::ok(crate::achievements::catalog(&ctx.db)?))
}

async fn data_export(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(ctx.db.export_all()?))
}

#[derive(Deserialize)]
struct ImportBody {
    payload: serde_json::Value,
    #[serde(default)]
    wipe: bool,
}

async fn data_import(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(body): Json<ImportBody>,
) -> ApiResult<serde_json::Value> {
    ensure_auth(&ctx, &headers)?;
    if body.wipe {
        ctx.db.wipe()?;
    }
    let mut imported = 0;
    if let Some(nodes) = body.payload["nodes"].as_array() {
        for n in nodes {
            let content = n["content"].as_str().unwrap_or("").to_string();
            if content.is_empty() {
                continue;
            }
            ctx.db.create_node(NewNode {
                content,
                date: n["date"].as_str().map(|s| s.to_string()),
                tags: n["tags"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                todo_id: None,
            })?;
            imported += 1;
        }
    }
    if let Some(todos) = body.payload["todos"].as_array() {
        for t in todos {
            let title = t["title"].as_str().unwrap_or("").to_string();
            if title.is_empty() {
                continue;
            }
            let todo = ctx.db.create_todo(NewTodo {
                title,
                description: t["description"].as_str().unwrap_or("").to_string(),
                due_date: t["dueDate"].as_str().map(String::from),
                due_time: t["dueTime"].as_str().map(String::from),
                priority: t["priority"].as_str().unwrap_or("中").to_string(),
                recur_type: String::new(),
                recur_until: String::new(),
                recur_interval: 1,
                recur_skip_rest: false,
                tags: t["tags"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                remind_offset_min: None,
                remind_at: t["remindAt"].as_str().map(String::from),
            })?;
            if t["status"].as_str() == Some("已完成") {
                ctx.db.complete_todo(todo.id, true)?;
            }
            imported += 1;
        }
    }
    if let Some(settings) = body.payload["settings"].as_array() {
        for s in settings {
            if let (Some(k), Some(v)) = (s["key"].as_str(), s["value"].as_str()) {
                ctx.db.set_setting(k, v)?;
            }
        }
    }
    ctx.bus.publish(Event::new("data.imported", json!({ "count": imported })));
    Ok(ApiResp::ok(json!({ "imported": imported })))
}

/// 全量数据导出为 Markdown 归档 ZIP（FR-7.6）
async fn data_export_markdown(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    ensure_auth(&ctx, &headers)?;
    let bytes = crate::export::generate_archive(&ctx.db)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let filename = format!(
        "mindmate-archive-{}.zip",
        chrono::Local::now().format("%Y%m%d")
    );
    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response())
}

// ───────────────────────── 推送渠道（FR-4.10）─────────────────────────

async fn push_config(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
) -> ApiResult<crate::push::PushConfig> {
    ensure_auth(&ctx, &headers)?;
    Ok(ApiResp::ok(crate::push::load_config(&ctx.db)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushConfigBody {
    config: crate::push::PushConfig,
    /// 凭据仅在用户输入新值时提交；空/缺省表示不修改
    smtp_password: Option<String>,
    telegram_token: Option<String>,
}

async fn push_save_config(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(body): Json<PushConfigBody>,
) -> ApiResult<crate::push::PushConfig> {
    ensure_auth(&ctx, &headers)?;
    crate::push::save_config(&ctx.db, &body.config)?;
    crate::push::save_credentials(
        body.smtp_password.as_deref(),
        body.telegram_token.as_deref(),
    )?;
    ctx.bus.publish(Event::new(
        "settings.updated",
        json!({ "keys": ["push_channels"] }),
    ));
    Ok(ApiResp::ok(crate::push::load_config(&ctx.db)))
}

#[derive(Deserialize)]
struct PushTestBody {
    channel: String,
}

async fn push_test(
    State(ctx): State<Arc<AppContext>>,
    headers: HeaderMap,
    Json(body): Json<PushTestBody>,
) -> ApiResult<crate::push::PushResult> {
    ensure_auth(&ctx, &headers)?;
    let r = crate::push::test_channel(&ctx.db, &body.channel).await;
    Ok(ApiResp::ok(r))
}

// ───────────────────────── SSE 事件流 ─────────────────────────

async fn stream_events(
    State(ctx): State<Arc<AppContext>>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<SseStream, ApiError> {
    // 支持 query 传 token（EventSource 无法自定义 header）
    if ctx.cfg.requires_login() {
        let token = q.get("token").cloned().unwrap_or_default();
        let ok = token == ctx.local_token || {
            use jsonwebtoken::{decode, DecodingKey, Validation};
            #[derive(serde::Deserialize)]
            struct Claims {
                sub: String,
                #[allow(dead_code)]
                exp: usize,
            }
            decode::<Claims>(
                &token,
                &DecodingKey::from_secret(ctx.jwt_secret.as_bytes()),
                &Validation::default(),
            )
            .is_ok()
        };
        if !ok {
            return Err(ApiError::unauthorized("未登录"));
        }
    }

    let mut rx = ctx.bus.subscribe();
    let stream = async_stream::stream! {
        // 首帧：连接确认
        yield Ok(SseEvent::default().event("ready").data(json!({"ok": true}).to_string()));
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let kind = ev.kind.clone();
                    let data = serde_json::to_string(&ev).unwrap_or_default();
                    yield Ok(SseEvent::default().event(kind).data(data));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Ok(boxed_sse(stream))
}


#[cfg(test)]
mod ai_tag_tests {
    use super::parse_tag_array;

    #[test]
    fn 解析_标准数组() {
        assert_eq!(parse_tag_array("[\"工作\",\"AI\"]"), vec!["工作", "AI"]);
    }

    #[test]
    fn 解析_容忍代码块包裹与废话() {
        let text = "好的，标签如下：\n```json\n[\"学习\", \"健康\"]\n```\n希望对你有帮助";
        assert_eq!(parse_tag_array(text), vec!["学习", "健康"]);
    }

    #[test]
    fn 解析_清洗_去重_限长限量() {
        // 空串剔除 / 井号前缀剥掉 / 超长剔除 / 去重 / 最多 3 个
        let text = r##"["", "工作", "#生活", "这是一个超过十二个字的超长标签不该被采纳", "学习", "健康", "娱乐"]"##;
        assert_eq!(parse_tag_array(text), vec!["工作", "生活", "学习"]);
    }

    #[test]
    fn 解析_不是数组就为空() {
        assert!(parse_tag_array("工作、生活").is_empty());
        assert!(parse_tag_array("").is_empty());
    }
}
