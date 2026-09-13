//! 内嵌前端资源：把 Vue 构建产物打包进二进制，由 Rust 核心的 axum 同源托管
//! —— 桌面端与浏览器端访问完全相同的 URL，无需外部文件与 CORS。

use mindmate_core::api::AssetResolver;
use rust_embed::RustEmbed;
use std::sync::Arc;

#[derive(RustEmbed)]
#[folder = "../apps/web/dist"]
struct WebAssets;

fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "map" => "application/json; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 构造资源解析器
pub fn resolver() -> AssetResolver {
    Arc::new(|path: &str| {
        let clean = path.trim_start_matches('/');
        let key = if clean.is_empty() { "index.html" } else { clean };
        WebAssets::get(key).map(|f| (mime_of(key).to_string(), f.data.into_owned()))
    })
}

/// 是否已内嵌前端产物（未构建前端时为 false，此时回退到 web_dir）
pub fn has_assets() -> bool {
    WebAssets::get("index.html").is_some()
}
