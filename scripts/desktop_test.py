#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
桌面端静态验收（Windows 打包前必跑）：守护两个"改错就白屏/功能全废"的关键点

背景（两个真实缺陷的回归护栏）：
  1) 桌面窗口加载的是本机 HTTP 页面（http://127.0.0.1:<端口>），在 Tauri 2 中属于
     remote 来源。capabilities 里若没有 remote.urls，ACL 会拒绝**所有** invoke：
     速记按钮打不开浮窗、设置面板的开机自启一律报错。普通接口测试发现不了，
     因为它只在桌面 WebView 里发生。
  2) 速记浮窗里调用 window.close() 会销毁 Windows/WebView2 的网页视图，只留下
     一个没有内容、没有按钮、Esc 也无效的白色空窗（无法关闭）。收起浮窗必须走
     close_quick_entry（隐藏窗口），并且 Alt+Z 要能再次按下收起（白屏时的逃生口）。

用法：python scripts/desktop_test.py
"""
import json
import os
import re
import sys

for s in (sys.stdout, sys.stderr):
    try:
        s.reconfigure(encoding="utf-8")
    except Exception:
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
passed, failed = [], []

# 应用自身命令（须同时出现在 build.rs 的 app manifest 与 capability 的 allow-* 权限里）
APP_COMMANDS = ["open_quick_entry", "close_quick_entry", "backend_port", "autostart_status", "autostart_set"]


def check(name, cond, detail=""):
    (passed if cond else failed).append(name)
    print(f"  [{'PASS' if cond else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))


def read(*parts):
    p = os.path.join(ROOT, *parts)
    with open(p, "r", encoding="utf-8") as f:
        return f.read()


def section(title):
    print(f"\n{title}")


# ── 1. Tauri 能力清单必须放行本机 HTTP 来源 ──
# ── 1. 桌面端 IPC 授权（capabilities + app manifest）──
section("1. 桌面端 IPC 授权（capabilities + app manifest）")
cap = json.loads(read("src-tauri", "capabilities", "default.json"))
main_rs = read("src-tauri", "src", "main.rs")
remote_urls = (cap.get("remote") or {}).get("urls") or []
perms = cap.get("permissions") or []
check("capability 声明了 remote.urls（否则桌面端 invoke 全部被 ACL 拒绝）", bool(remote_urls), str(remote_urls))
check(
    "放行 http://127.0.0.1:<任意端口>（端口会因占用自增，不能写死）",
    any(u.replace(" ", "").startswith(("http://127.0.0.1:*", "http://localhost:*")) for u in remote_urls),
    str(remote_urls),
)
check("main / quick 两个窗口都在授权范围内", set(cap.get("windows") or []) >= {"main", "quick"}, str(cap.get("windows")))
missing_allow = [c for c in APP_COMMANDS if f"allow-{c.replace('_', '-')}" not in perms]
check(
    "capability 为应用自身命令声明 allow-* 权限（不带插件前缀）",
    not missing_allow,
    "缺失: " + str(missing_allow),
)

build_rs = read("src-tauri", "build.rs")
missing_manifest = [c for c in APP_COMMANDS if f'"{c}"' not in build_rs]
check(
    "build.rs 声明 app manifest 命令清单（未声明则 remote 来源的 invoke 一律被拒）",
    "AppManifest::new().commands" in build_rs and not missing_manifest,
    "缺失: " + str(missing_manifest),
)
check("app manifest 走 try_build（权限写错在构建期就报错，不静默失败）", "try_build" in build_rs)
handler = main_rs.split("generate_handler![", 1)[-1].split("]", 1)[0] if "generate_handler![" in main_rs else ""
for c in APP_COMMANDS:
    check(f"命令 {c} 已注册进 invoke_handler", c in handler)
for p in ("autostart:allow-enable", "core:window:allow-hide", "core:event:default"):
    check(f"保留插件权限 {p}", p in perms)

# ── 2. 前端不得销毁 WebView2 视图 ──
section("2. 速记浮窗的收起方式（前端）")
src_files = []
for base, _dirs, files in os.walk(os.path.join(ROOT, "apps", "web", "src")):
    for f in files:
        if f.endswith((".vue", ".ts")):
            src_files.append(os.path.join(base, f))
offenders = []
for p in src_files:
    with open(p, "r", encoding="utf-8") as f:
        for i, line in enumerate(f, 1):
            if re.search(r"window\s*\.\s*close\s*\(", line) and not line.strip().startswith(("//", "*")):
                offenders.append(f"{os.path.relpath(p, ROOT)}:{i}")
check("前端不存在 window.close()（WebView2 上会留下无法关闭的白屏空窗）", not offenders, ", ".join(offenders))

quick = read("apps", "web", "src", "views", "Quick.vue")
check("Quick.vue 收起浮窗调用 closeQuickEntry", "closeQuickEntry" in quick)
check("Quick.vue 在浮窗重新唤起时复位状态并聚焦（focus / visibilitychange）",
      "addEventListener('focus'" in quick and "visibilitychange" in quick)
check("Quick.vue 支持 Esc 收起", "Escape" in quick)
check("Quick.vue 自动收起走 closeWindow（不残留白窗）", "await closeWindow()" in quick)

desktop_ts = read("apps", "web", "src", "lib", "desktop.ts")
check("openQuickEntry 以布尔返回值判断是否由桌面端接管（返回 unit 会被误判为失败）",
      "=== true" in desktop_ts and "open_quick_entry" in desktop_ts)

# ── 3. Rust 侧窗口生命周期 ──
section("3. 速记浮窗的 Rust 侧生命周期")
main_rs = read("src-tauri", "src", "main.rs")
check("注册 open_quick_entry / close_quick_entry 命令",
      "open_quick_entry," in main_rs and "close_quick_entry," in main_rs)
check("open_quick_entry 返回布尔（前端据此跳过浏览器兜底）",
      re.search(r"fn open_quick_entry\(app: tauri::AppHandle\) -> bool", main_rs) is not None)
check("存在 toggle_quick（再次按 Alt+Z 收起，白屏时也能关）", "fn toggle_quick" in main_rs)
check("全局热键 Alt+Z 走 toggle_quick", re.search(r"ShortcutState::Pressed \{\s*toggle_quick", main_rs) is not None)
check("托盘速记菜单走 toggle_quick", '"quick" => toggle_quick(app)' in main_rs)
check("浮窗关闭请求降级为隐藏（prevent_close），窗口不会被销毁",
      "CloseRequested" in main_rs and "api.prevent_close()" in main_rs)
check("浮窗失焦自动收起", "WindowEvent::Focused(false)" in main_rs)
check("不再对浮窗 eval 注入脚本（未就绪时抛 WebView2 0x8007139F）",
      "mindmate:focus-quick-entry" not in main_rs)
check("浮窗为无边框 + 置顶 + 不进任务栏",
      "decorations(false)" in main_rs and "always_on_top(true)" in main_rs and "skip_taskbar(true)" in main_rs)

# ── 4. 自动更新与发布通道（防止误删导致"发出去但用户更新不了"）──
section("4. 自动更新与发布通道")
conf = json.loads(read("src-tauri", "tauri.conf.json"))
updater = (conf.get("plugins") or {}).get("updater") or {}
pubkey = updater.get("pubkey", "")
check("tauri.conf.json 配置了更新公钥（缺失则客户端拒绝安装更新包）", len(pubkey) > 40 and "REPLACE" not in pubkey)
check("配置了更新清单地址 endpoints", bool(updater.get("endpoints")), str(updater.get("endpoints"))[:60])
check("开启 createUpdaterArtifacts（否则构建不出 .sig 签名文件）",
      (conf.get("bundle") or {}).get("createUpdaterArtifacts") is True)
cargo = read("src-tauri", "Cargo.toml")
check("依赖 tauri-plugin-updater", "tauri-plugin-updater" in cargo)
check("依赖 tauri-plugin-process（更新后重启用）", "tauri-plugin-process" in cargo)
check("main.rs 注册 updater 与 process 插件",
      "tauri_plugin_updater::Builder::new().build()" in main_rs and "tauri_plugin_process::init()" in main_rs)
check("capability 放行 updater / process 权限", "updater:default" in perms and "process:default" in perms)
gi = read(".gitignore")
check(".gitignore 排除签名私钥与密钥目录（绝不能入库）",
      ".tauri/" in gi and "*.key" in gi and "*.pem" in gi)
wf = read(".github", "workflows", "release.yml")
check("流水线用 Secrets 中的私钥签名", "TAURI_SIGNING_PRIVATE_KEY" in wf)
check("流水线部署到 Cloudflare Pages", "pages deploy" in wf and "CLOUDFLARE_API_TOKEN" in wf)
check("流水线注入 RELEASE_BASE_URL（清单下载地址来源）", "RELEASE_BASE_URL" in wf)
check("流水线先跑 Rust 单测再出包", "cargo test" in wf)
check("存在清单生成脚本（latest.json / 校验文件 / 下载页）",
      os.path.exists(os.path.join(ROOT, "scripts", "make-manifest.mjs")))
check("存在绿色版打包脚本", os.path.exists(os.path.join(ROOT, "scripts", "make-portable.mjs")))
check("发布文档已就位", os.path.exists(os.path.join(ROOT, "docs", "发布与更新文档.md")))

print("\n" + "=" * 60)
print(f"桌面端静态验收：通过 {len(passed)} 项，失败 {len(failed)} 项")
for f in failed:
    print(f"  - {f}")
print("=" * 60)
sys.exit(1 if failed else 0)
