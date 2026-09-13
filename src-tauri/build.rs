/// 桌面端构建脚本。
///
/// 必须显式声明应用自身命令的清单（app manifest）：桌面窗口加载的是本机 HTTP 页面
/// （http://127.0.0.1:<端口>），在 Tauri 2 中属于 remote 来源，未在清单里的命令
/// 会被 ACL 直接拒绝（前端只看到 "Command xxx not allowed by ACL"）——
/// 表现为速记按钮打不开浮窗、设置里开机自启必然报错。
///
/// 清单里的每条命令会自动生成 allow-<命令名-中划线> / deny-<...> 权限，
/// 供 capabilities/default.json 引用（应用自身命令不带插件前缀）。
fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "open_quick_entry",
                "close_quick_entry",
                "backend_port",
                "autostart_status",
                "autostart_set",
            ]),
        ),
    )
    .expect("tauri-build 执行失败");
}
