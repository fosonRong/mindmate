//! 智伴 Mindmate 桌面端（Tauri 2）
//!
//! 单一进程同时承担：桌面壳（窗口/托盘/热键/通知）+ Rust 核心（HTTP/SSE/提醒调度/AI）。
//! 主窗口加载本机 HTTP 地址 —— 与浏览器端访问同一份前端与同一套 API。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod autostart;

use mindmate_core::config::AppConfig;
use mindmate_core::reminder;
use mindmate_core::AppContext;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_notification::NotificationExt;

/// 后端实际监听端口（供前端与窗口加载使用）
static PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(17801);

fn port() -> u16 {
    PORT.load(std::sync::atomic::Ordering::SeqCst)
}

fn base_url(path: &str) -> String {
    format!("http://127.0.0.1:{}{}", port(), path)
}

/// 显示主窗口
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 速记浮窗是否可见
fn quick_visible(app: &tauri::AppHandle) -> bool {
    app.get_webview_window("quick")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// 显示速记浮窗（全局热键 / 托盘 / 前端命令）
fn show_quick(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("quick") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// 隐藏速记浮窗（只隐藏不销毁，WebView 保持存活）
fn hide_quick(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("quick") {
        let _ = w.hide();
    }
}

/// 切换速记浮窗：可见则收起。
/// 这是白屏时的兜底逃生通道 —— 页面脚本一旦出错，浮窗内既没有"关闭"按钮也收不到 Esc，
/// 只有再按一次 Alt+Z（或托盘菜单）才能关掉它。
fn toggle_quick(app: &tauri::AppHandle) {
    if quick_visible(app) {
        hide_quick(app);
    } else {
        show_quick(app);
    }
}

// ───────────────────────── Tauri 命令（供前端 invoke） ─────────────────────────

/// 打开速记浮窗。返回 true 表示桌面端已接管，前端无需再走浏览器兜底。
#[tauri::command]
fn open_quick_entry(app: tauri::AppHandle) -> bool {
    show_quick(&app);
    true
}

#[tauri::command]
fn close_quick_entry(app: tauri::AppHandle) {
    hide_quick(&app);
}

#[tauri::command]
fn backend_port() -> u16 {
    port()
}

/// 查询开机自启状态（桌面端）
#[tauri::command]
fn autostart_status() -> Result<bool, String> {
    autostart::is_enabled(autostart::ENTRY_NAME).map_err(|e| e.to_string())
}

/// 开启/关闭开机自启（桌面端）
#[tauri::command]
fn autostart_set(enabled: bool) -> Result<bool, String> {
    if enabled {
        autostart::enable(autostart::ENTRY_NAME).map_err(|e| format!("开启开机自启失败：{e}"))?;
    } else {
        autostart::disable(autostart::ENTRY_NAME).map_err(|e| format!("关闭开机自启失败：{e}"))?;
    }
    autostart::is_enabled(autostart::ENTRY_NAME).map_err(|e| e.to_string())
}

/// 容器健康检查：--healthcheck 时请求本机 healthz，成功退出码 0
fn healthcheck() -> ! {
    let cfg = AppConfig::from_args_and_env();
    let url = format!("http://127.0.0.1:{}/api/v1/healthz", cfg.port);
    let ok = (0..10).any(|_| {
        match std::net::TcpStream::connect(("127.0.0.1", cfg.port)) {
            Ok(mut stream) => {
                use std::io::{Read, Write};
                let req = "GET /api/v1/healthz HTTP/1.0
Host: 127.0.0.1

";
                let _ = stream.write_all(req.as_bytes());
                let mut buf = String::new();
                let _ = stream.read_to_string(&mut buf);
                buf.contains("\"status\":\"ok\"")
            }
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                false
            }
        }
    });
    let _ = url;
    std::process::exit(if ok { 0 } else { 1 });
}

fn main() {
    if std::env::args().any(|a| a == "--healthcheck") {
        healthcheck();
    }

    // 日志（写入数据目录 logs/, 同时输出到控制台）
    let cfg = AppConfig::from_args_and_env();
    let _ = std::fs::create_dir_all(cfg.log_dir());
    let file_appender = tracing_appender::rolling::daily(cfg.log_dir(), "mindmate.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("MINDMATE_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(non_blocking)
        .init();

    tracing::info!("智伴 Mindmate 启动中… 模式={} 数据目录={:?}", cfg.mode.as_str(), cfg.data_dir);

    // 1) 启动 Rust 核心（HTTP + 事件总线 + 提醒调度），与桌面壳同进程
    let ctx = match AppContext::new(cfg.clone()) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("初始化核心失败: {e}");
            eprintln!("智伴启动失败：{e}");
            return;
        }
    };

    let assets = if assets::has_assets() { Some(assets::resolver()) } else { None };
    if assets.is_none() {
        tracing::warn!("未检测到内嵌前端产物（apps/web/dist），将回退到 --web-dir");
    }

    // 在独立 tokio 运行时中启动服务（与 Tauri 的异步运行时共存）
    let rt = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");
    let ctx_for_server = ctx.clone();
    let port = rt
        .block_on(async move {
            let p = mindmate_core::api::serve_with_assets(ctx_for_server, None, assets).await?;
            Ok::<u16, anyhow::Error>(p)
        })
        .unwrap_or_else(|e| {
            tracing::error!("启动 HTTP 服务失败: {e}");
            17801
        });
    PORT.store(port, std::sync::atomic::Ordering::SeqCst);
    tracing::info!("核心服务监听于 http://127.0.0.1:{port}");

    // 2) 启动提醒调度器
    let ctx_for_reminder = ctx.clone();
    rt.spawn(async move {
        reminder::spawn(ctx_for_reminder);
    });
    // 让运行时保持存活（后台线程持有 runtime 并永久阻塞）
    std::thread::spawn(move || {
        rt.block_on(async {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });
    });

    // 服务器模式（--headless）：仅运行后端，供 Docker / 云主机部署
    if cfg.mode == mindmate_core::RunMode::Server {
        tracing::info!("无窗口服务器模式：仅提供 HTTP 服务（端口 {port}），Ctrl+C 退出");
        std::thread::park();
        return;
    }

    let ctx_for_app = ctx.clone();

    // 3) 桌面壳
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            open_quick_entry,
            close_quick_entry,
            backend_port,
            autostart_status,
            autostart_set
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // ── 主窗口：加载本机 HTTP 服务（与浏览器端同一份前端）──
            let main_url: tauri::Url = base_url("/").parse()?;
            let main_window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(main_url))
                .title("智伴 Mindmate")
                .inner_size(1200.0, 820.0)
                .min_inner_size(880.0, 600.0)
                .center()
                .build()?;

            // 关闭窗口 = 隐藏到托盘（提醒继续生效）
            let handle_for_close = handle.clone();
            main_window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Some(w) = handle_for_close.get_webview_window("main") {
                        let _ = w.hide();
                    }
                }
            });

            // ── 速记浮窗（默认隐藏，热键唤出）──
            let quick_url: tauri::Url = base_url("/#/quick").parse()?;
            let _quick = WebviewWindowBuilder::new(app, "quick", WebviewUrl::External(quick_url))
                .title("速记")
                .inner_size(380.0, 216.0)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .center()
                .visible(false)
                .build()?;
            let handle_for_blur = handle.clone();
            if let Some(q) = app.get_webview_window("quick") {
                q.on_window_event(move |event| match event {
                    // 失焦即收起（浮窗语义）
                    tauri::WindowEvent::Focused(false) => hide_quick(&handle_for_blur),
                    // 关闭请求一律降级为隐藏：窗口与 WebView 始终保持存活，避免被销毁后重建
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        hide_quick(&handle_for_blur);
                    }
                    _ => {}
                });
            }

            // ── 托盘 ──
            let show_item = MenuItem::with_id(app, "show", "打开智伴", true, None::<&str>)?;
            let quick_item = MenuItem::with_id(app, "quick", "速记 (Alt+Z)", true, None::<&str>)?;
            let pause_item = CheckMenuItem::with_id(app, "pause", "暂停提醒", true, false, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quick_item, &pause_item, &quit_item])?;

            let pause_item_for_menu = pause_item.clone();
            let ctx_for_tray = ctx_for_app.clone();
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().cloned().ok_or("缺少应用图标")?)
                .tooltip("智伴 Mindmate")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quick" => toggle_quick(app),
                    "pause" => {
                        let paused = pause_item_for_menu.is_checked().unwrap_or(false);
                        let _ = ctx_for_tray
                            .db
                            .set_setting("remind_enabled", if paused { "0" } else { "1" });
                        let _ = pause_item_for_menu.set_checked(paused);
                        ctx_for_tray.bus.publish(mindmate_core::Event::new(
                            "settings.updated",
                            serde_json::json!({ "keys": ["remind_enabled"] }),
                        ));
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            // ── 全局热键 Alt+Z → 速记浮窗（再次按下收起） ──
            use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
            let shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyZ);
            let handle_for_shortcut = handle.clone();
            let _ = app.global_shortcut().on_shortcut(shortcut, move |_app, _sc, event| {
                if event.state() == ShortcutState::Pressed {
                    toggle_quick(&handle_for_shortcut);
                }
            });
            tracing::info!("全局热键已注册：Alt+Z");

            // ── 核心事件 → 桌面系统通知 ──
            let mut rx = ctx_for_app.bus.subscribe();
            let handle_for_notify = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            if ev.kind != "reminder.triggered" {
                                continue;
                            }
                            let title = ev.payload["title"].as_str().unwrap_or("智伴").to_string();
                            let body = ev.payload["body"].as_str().unwrap_or("").to_string();
                            let action = ev.payload["action"].as_str().unwrap_or("").to_string();
                            let _ = handle_for_notify
                                .notification()
                                .builder()
                                .title(title)
                                .body(body)
                                .show();
                            // 晨间简报 / 晚安总结 → 唤起主窗口
                            if action == "open_today" {
                                show_main(&handle_for_notify);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            });

            tracing::info!("桌面壳就绪（托盘 + 热键 + 通知）");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("智伴桌面端运行失败");

    // 保持 ctx 存活到进程结束
    drop(ctx);
}
