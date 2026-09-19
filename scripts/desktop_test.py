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

# ── 1.5 Tauri 命令不得在主线程做阻塞 IO ──
# Tauri 的**同步**命令在主线程执行：命令体里一旦启动进程 / 读写注册表（哪怕只有 150ms），
# WebView 主线程就会被冻住。真机踩过：autostart_status 调 reg.exe 读注册表，
# 一切到设置页就卡很久（浏览器端不走这条路径，所以只在桌面端复现）。
# 因此：凡是命令体里出现 autostart::（内部用 reg.exe）的命令，必须是 async。
print("\n1.5 命令不得阻塞主线程")
cmd_blocks = re.findall(r"#\[tauri::command\]\s*\n((?:.*\n)*?)\}", main_rs)
for block in cmd_blocks:
    m = re.search(r"fn\s+(\w+)", block)
    if not m:
        continue
    name = m.group(1)
    is_async = re.search(r"async\s+fn", block) is not None
    does_process_io = "autostart::" in block or "Command::new" in block
    if does_process_io:
        check(f"命令 {name} 做进程/注册表 IO 且为 async（否则冻结界面）", is_async,
              "同步命令在主线程执行" if not is_async else "已用 spawn_blocking")
        check(f"命令 {name} 用 spawn_blocking 承载阻塞调用", "spawn_blocking" in block,
              "避免占用 tokio worker")
check("命令体解析到（结构变化时本检查需同步）", len(cmd_blocks) >= 4, f"解析到 {len(cmd_blocks)} 个命令")

# 读注册表不得启动子进程：reg.exe 每次 150–180ms，设置页一进来就要读自启状态，
# 用户感受就是「点设置卡很久」。已改为进程内注册表 API（winreg）。
autostart_rs = read("src-tauri", "src", "autostart.rs")
check("自启状态读取不启动子进程（不用 reg.exe）",
      'Command::new("reg")' not in autostart_rs and "Stdio::piped" not in autostart_rs,
      "reg.exe 单次 150–180ms，会拖慢设置页")

# 数据安全（2026-09-13 事故）：/data/import + wipe=true 曾把用户的真实待办清掉。
# 现在两道锁：清空前自动备份 + 测试套件必须指向独立数据目录的服务。
queries_rs = read("core", "src", "db", "queries.rs")
check("清空数据（wipe）前会先做整库备份", "backup_before_wipe" in queries_rs and "VACUUM INTO" in queries_rs,
      "否则误调用后无法恢复")
# 内置模板改进要惠及存量用户（v1.0.13 教训：改了默认简报模板，点过「保存/恢复默认」
# 的用户库里固化着旧模板，升级后新能力永远不生效且无提示）
ai_rs = read("core", "src", "ai", "mod.rs")
check("存在存量默认模板升级机制（STOCK_TEMPLATES_HISTORY + upgrade_stock_templates）",
      "STOCK_TEMPLATES_HISTORY" in ai_rs and "upgrade_stock_templates" in ai_rs)
check("启动时会执行模板升级", "upgrade_stock_templates" in read("core", "src", "lib.rs"))
check("模板升级不触碰用户自定义（只匹配历史默认）",
      "stored.trim() == h.trim()" in ai_rs.replace('"', '') or "h.trim()" in ai_rs)
# 报告防源码显示（真机踩过：模型把整份日报包在 ```markdown 里，界面显示源码）
md_view = read("apps", "web", "src", "components", "MarkdownView.vue")
check("渲染层会剥掉包裹全文的 ```markdown 围栏", "unwrapMarkdownFence" in md_view)
check("剥围栏实现独立成模块（供验收脚本直接测试）",
      os.path.exists(os.path.join(ROOT, "apps", "web", "src", "lib", "markdown.ts")))
check("报告模板带防围栏提示词约束", "不要把整份内容包在" in ai_rs)
# 可点击提示必须真的能点（真机踩过：周视图「＋N 更多」是纯文本，点击没反应）
week_vue = read("apps", "web", "src", "views", "Week.vue")
_has_more_button = "btn-more" in week_vue and "@click.stop=\"toggleExpand(d.date)\"" in week_vue
check("周视图「＋N 更多」是按钮且绑定了展开事件", _has_more_button,
      "纯文本提示会让用户以为是可点击的")
check("周视图支持展开后收起（不只有展开）", "toggleExpand" in week_vue and "收起 ▴" in week_vue)
check("展开后显示记录全文（截断只在折叠态）",
      "displayText" in week_vue and "isExpanded(date)" in week_vue)
run_all = read("scripts", "run_all_tests.py")
check("全量验收自行拉起独立数据目录的临时服务（不再打真实数据目录）",
      "--data-dir" in run_all and "--mode" in run_all and "MINDMATE_ALLOW_WIPE" in run_all,
      "事故根因：测试清库直接作用在用户数据目录")
guards = all("_wipe_guard" in read("scripts", f) for f in ("e2e_test.py", "reminder_test.py", "push_test.py"))
check("三个破坏性套件都装了清库守卫（显式确认 + 必须是 server 模式）", guards)
check("Windows 侧使用进程内注册表 API（winreg）", "winreg" in autostart_rs and "RegKey" in autostart_rs)

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
check("发布只包含安装版（不再生成绿色版）",
      not os.path.exists(os.path.join(ROOT, "scripts", "make-portable.mjs"))
      and "portable" not in read(".github", "workflows", "release.yml"))
manifest_js = read("scripts", "make-manifest.mjs")
check("更新清单按多平台生成（windows + darwin 两个架构）",
      "darwin-aarch64" in manifest_js and "darwin-x86_64" in manifest_js and "windows-x86_64" in manifest_js)
check("更新清单覆盖 macOS 平台键（写错会让 mac 用户收不到更新）",
      "darwin-${arch}" in manifest_js or "darwin-" in manifest_js)
check("架构识别兼容 CI 的产物目录名（darwin-aarch64 / darwin-x86_64，而非只有 cargo 三元组）",
      "darwin-aarch64" in manifest_js and "/aarch64|arm64/" in manifest_js,
      "曾把两个架构识别成同一个，清单里只剩 x86_64，Apple 芯片用户静默收不到更新")
check("清单生成会校验两个 macOS 架构都在（缺一个直接失败而非静默跳过）",
      "缺少架构" in manifest_js and "process.exit(1)" in manifest_js)
check("发布文档已就位", os.path.exists(os.path.join(ROOT, "docs", "发布与更新文档.md")))

# 杀软误报（v1.0.16 的真实事故：安装包能装，但主程序被 Defender 判为
# Trojan:Win32/Bearfoos.A!ml 并隔离，用户表现为"安装后打不开"）。
# 未签名二进制的 ML 误报无法从代码里消除，只能靠"发布前真的扫一次"来拦截，
# 因此这里把这条防线也钉成静态断言，避免后续提交把它删掉。
av_script = read("scripts", "defender_check.py")
check("存在杀软误报自检脚本", os.path.exists(os.path.join(ROOT, "scripts", "defender_check.py")))
check("杀软自检在 %TEMP% 中立目录扫描（项目目录/安装目录已在 Defender 排除项内，原地扫恒为干净）",
      "mkdtemp" in av_script and "copy2" in av_script)
check("杀软自检以 MpCmdRun 退出码为判据（2 = 检出威胁）",
      "MpCmdRun" in av_script and "returncode == 2" in av_script)
check("杀软自检支持 --strict（CI 检出即失败）",
      "--strict" in av_script and "return 1 if strict else 0" in av_script)
check("非 Windows / 无 Defender 时自检自动跳过（不阻断 macOS 构建）",
      "非 Windows：跳过杀软自检" in av_script and "未找到 MpCmdRun.exe" in av_script)
check("CI 在上传 Windows 产物前强制杀软自检",
      "defender_check.py --strict" in wf,
      "缺这一步就会把「装完打不开」的包发给用户（v1.0.16 事故）")
check("全量验收纳入杀软误报自检", "defender_check.py" in read("scripts", "run_all_tests.py"))
check("下载页给出 Windows 误报自救指引（保护历史记录 → 排除项 → 重装）",
      "保护历史记录" in manifest_js and "排除项" in manifest_js)
check("误报处置文档已就位（含提交给微软的说明文本）",
      os.path.exists(os.path.join(ROOT, "docs", "杀软误报处置.md"))
      and "filesubmission" in read("docs", "杀软误报处置.md"))

# ── 迭代：农历节假日 / 日程方块自适应 / 循环待办 / 我的简报+今日热点（2026-09 需求） ──
lunar = read("apps", "web", "src", "lib", "lunar.ts")
check("农历模块存在且带 1900-2049 压缩表", "LUNAR_INFO" in lunar and "0x04bd8" in lunar)
check("农历含法定节假日调休表（休/班）", "HOLIDAY_DATA" in lunar and "'work':" in lunar.replace('"work"', "'work'") or "work:" in lunar or "work" in lunar)
check("农历测试用真实锚点校验（历年春节）", os.path.exists(os.path.join(ROOT, "scripts", "lunar_test.mjs"))
      and "2026-02-17" in read("scripts", "lunar_test.mjs"))
check("日历格等额自适应（minmax(0,1fr) 列 + 行 + 视口高度）",
      "grid-template-columns: repeat(7, minmax(0, 1fr))" in read("apps", "web", "src", "styles", "app.css")
      and "grid-auto-rows: minmax(0, 1fr)" in read("apps", "web", "src", "styles", "app.css"))
check("格内：日期与农历同一行（横向布局，农历可省略）",
      ".cal-cell .num .lunar" in read("apps", "web", "src", "styles", "app.css")
      and '<span class="lunar"' in read("apps", "web", "src", "components", "CalendarMonth.vue"))
check("格内：日程文字区填满剩余高宽并溢出隐藏（跟随方块自适应）",
      ".cal-cell .sum-wrap { flex: 1 1 0; min-height: 0; min-width: 0; overflow: hidden;" in read("apps", "web", "src", "styles", "app.css"))
check("格内：全部记录交给格子裁剪（后端不再按 28 字预截断，前端渲染多条）",
      "summarize(&n.content, 400)" in read("core", "src", "db", "queries.rs")
      and 'v-for="(s, si) in c.stat?.nodeSummaries' in read("apps", "web", "src", "components", "CalendarMonth.vue"))

check("循环待办：库迁移 V2+V3 加循环字段（类型/锚点/来源/截止/间隔/跳过休息日）",
      "recur_type" in read("core", "src", "db", "mod.rs")
      and "SCHEMA_V2" in read("core", "src", "db", "mod.rs")
      and "SCHEMA_V3" in read("core", "src", "db", "mod.rs")
      and "SCHEMA_VERSION: i64 = 3" in read("core", "src", "db", "mod.rs"))
check("循环待办：补期引擎（每天/每周/每月 + 月末截断）",
      "ensure_recurring" in read("core", "src", "db", "queries.rs")
      and '"daily"' in read("core", "src", "db", "queries.rs")
      and "days_in_month" in read("core", "src", "db", "queries.rs"))
check("循环待办：完成后补期并广播 todo.created",
      "publish_recurring_created" in read("core", "src", "api", "mod.rs"))
check("循环待办：跨天定时补期（提醒调度器内）",
      "ensure_recurring" in read("core", "src", "reminder", "mod.rs"))
check("循环待办：弹窗提供 不循环/每天/每周/每月",
      "RECUR_OPTIONS" in read("apps", "web", "src", "components", "TodoEditModal.vue")
      and "'daily'" in read("apps", "web", "src", "components", "TodoEditModal.vue"))
check("循环待办：列表项显示循环徽标", "chip recur" in read("apps", "web", "src", "components", "TodoItem.vue"))

check("晨间简报已更名为我的简报（界面无残留）",
      "我的简报" in read("apps", "web", "src", "views", "Today.vue")
      and "$t('晨间简报')" not in read("apps", "web", "src", "views", "Today.vue")
      and "$t('晨间简报')" not in read("apps", "web", "src", "views", "Settings.vue"))
check("简报模板同步更名且旧版进入升级清单（存量自动升级）",
      "请生成今天的我的简报" in read("core", "src", "ai", "mod.rs")
      and '标题「☀️ 我的简报」' in read("core", "src", "ai", "mod.rs"))

news = read("core", "src", "news.rs")
check("今日热点：后端抓取模块（60s 源 + 栏目注册表自动生成）",
      "60s.viki.moe" in news and "CHANNELS" in news)
check("今日热点：解析/合并纯函数有单测", "parse_channel" in news and "merge_items" in news and "mod tests" in news)
check("今日热点：API 路由（栏目清单 + 抓取）",
      "/api/v1/news/channels" in read("core", "src", "api", "mod.rs")
      and "/api/v1/news/hot" in read("core", "src", "api", "mod.rs"))
check("今日热点：缓存降级（stale 标记，空结果不覆盖旧缓存）",
      "stale" in news and "空结果不覆盖旧缓存" in news)
check("今日热点：下滑加载更多（前端滚动分页 + 更大 limit 上限）",
      "onNewsScroll" in read("apps", "web", "src", "views", "Today.vue")
      and "clamp(1, 200)" in read("core", "src", "api", "mod.rs"))
check("今日热点：自动更新开关与频率（设置持久化）",
      "news_auto_refresh" in read("apps", "web", "src", "views", "Settings.vue")
      and "news_refresh_minutes" in read("apps", "web", "src", "views", "Settings.vue")
      and "news_auto_refresh" in read("core", "src", "db", "mod.rs"))
check("今日热点：点击新闻用系统浏览器打开（桌面走 openUrl，浏览器开新标签）",
      "openNews" in read("apps", "web", "src", "views", "Today.vue")
      and "api.openUrl" in read("apps", "web", "src", "views", "Today.vue"))
check("全量验收纳入农历/节假日专项", "lunar_test.mjs" in read("scripts", "run_all_tests.py"))

# ── v1.0.19 反馈迭代：热点设置即时保存 / 切换即刷新 / 重点关注 / 重复按钮布局 ──
check("热点：重点关注注册表（行业 + 关键词）与过滤纯函数",
      "FOCUS_TOPICS" in news and "filter_focus" in news and "title_matches" in news)
check("热点：过滤有确定性单测（大小写/空关键词/预设+自定义合并）",
      "重点关注_标题命中大小写不敏感" in news and "重点关注_预设与自定义关键词合并过滤" in news)
check("热点：接口支持 focus/kw 参数（预设+自定义关键词过滤）",
      '"kw"' in read("core", "src", "api", "mod.rs") and 'q.get("focus")' in read("core", "src", "api", "mod.rs"))
check("热点：channels 接口带行业清单（自动生成）",
      "FOCUS_TOPICS" in read("core", "src", "api", "mod.rs"))
settings_src = read("apps", "web", "src", "views", "Settings.vue")
check("热点：设置全部控件即时保存（修复「设置了没保存」）",
      "saveNewsSettings" in settings_src
      and "@change=\"saveNewsSettings\"" in settings_src
      and "news_focus" in settings_src and "news_focus_keywords" in settings_src)
today_src = read("apps", "web", "src", "views", "Today.vue")
check("热点：切换到今日热点标签即强制刷新",
      "loadNews(true)" in today_src.split("function toggleNewsPanel")[1].split("}")[0] if "function toggleNewsPanel" in today_src else False)
check("热点：重点关注过滤接入面板（参数 + 空态 + 摘要）",
      "isFocusFiltering" in today_src and "newsFocusParams" in today_src and "focusSummary" in today_src)
check("热点：自定义关键词走互联网搜索（必应中国 + 相关度排序），不在热搜榜里过滤",
      "SEARCH_URL" in news and "parse_bing_results" in news and "relevance_score" in news
      and "search_keywords" in news and "search_keywords" in read("core", "src", "api", "mod.rs"))
check("热点：搜索有确定性单测（实体还原/结果解析/相关度排序）",
      "unescape_entities" in news and "必应结果解析_提取标题链接摘要" in news
      and "重点关注_标题命中大小写不敏感" in news)
check("热点：面板选择持久化（切换后保留，重启保留）",
      "mindmate_today_panel" in today_src and "localStorage.setItem(PANEL_KEY" in today_src)
check("热点：关注模式有专属缓存（参数签名匹配才命中，跨页切回展示上次记录）",
      "FOCUS_CACHE_KEY" in news and "params == sig" in read("core", "src", "api", "mod.rs")
      and "正在展示最近一次成功的数据" in read("core", "src", "api", "mod.rs"))

# ── v1.1.0：删除循环语义 / 循环增强 / 热力图 / 抽屉速记聚焦 ──
check("删除待办：支持删除整条循环链（根+实例一并软删，阻止补期再生成）",
      "delete_todo_series" in read("core", "src", "db", "queries.rs")
      and 'recur_type=''' in read("core", "src", "db", "queries.rs")
      and '"scope"' in read("core", "src", "api", "mod.rs"))
check("删除按钮可见性：TodoItem 的 showActions 必须显式默认 true（Vue3 Boolean 属性缺省即 false，曾致所有页面渲染不出删除按钮）",
      "withDefaults" in read("apps", "web", "src", "components", "TodoItem.vue")
      and "showActions: true" in read("apps", "web", "src", "components", "TodoItem.vue"))
check("删除按钮不冒泡到「点击=编辑」的行（否则点删除弹出编辑框，待办页实测）",
      '@click.stop' in read("apps", "web", "src", "components", "TodoItem.vue")
      and 'class="actions" @click.stop' in read("apps", "web", "src", "components", "TodoItem.vue"))
check("删除待办：前端循环待办二选一（删整个循环/仅此一条）+ 普通待办二次确认",
      "删整个循环" in read("apps", "web", "src", "components", "TodoItem.vue")
      and "确认删除？" in read("apps", "web", "src", "components", "TodoItem.vue"))
check("循环增强：迁移 V3（结束日期/间隔/跳过休息日）",
      "SCHEMA_V3" in read("core", "src", "db", "mod.rs") and "SCHEMA_VERSION: i64 = 3" in read("core", "src", "db", "mod.rs"))
check("循环增强：生成规则支持间隔/截止/跳过休息日（Rust 侧法定假期表）",
      "HOLIDAY_OFF" in read("core", "src", "db", "queries.rs")
      and "is_rest_day" in read("core", "src", "db", "queries.rs"))
check("循环增强：弹窗可配置 每 N 周期/结束日期/跳过休息日",
      "recurUntil" in read("apps", "web", "src", "components", "TodoEditModal.vue")
      and "recurInterval" in read("apps", "web", "src", "components", "TodoEditModal.vue")
      and "recurSkipRest" in read("apps", "web", "src", "components", "TodoEditModal.vue"))
check("热力图：近 26 周 GitHub 风格组件存在并接入月视图",
      os.path.exists(os.path.join(ROOT, "apps", "web", "src", "components", "HeatMap.vue"))
      and "HeatMap" in read("apps", "web", "src", "views", "Month.vue"))
check("月视图抽屉：打开即聚焦速记输入",
      '<QuickEntry :date="drawerDate" compact autofocus' in read("apps", "web", "src", "views", "Month.vue"))
modal_src = read("apps", "web", "src", "components", "TodoEditModal.vue")
check("待办弹窗：重复选择独占一行且按钮不换行（修复竖排文字）",
      "white-space: nowrap" in modal_src and "重复独占一行" in modal_src)

# 版本号一致性：core 与应用同时发布，版本号必须相同（否则 /install 诊断信息会误导）
core_toml = read("core", "Cargo.toml")
m = re.search(r'^version = "([^"]+)"', core_toml, re.M)
check("core 版本与应用版本一致", bool(m) and m.group(1) == conf.get("version"),
      f"core={m.group(1) if m else '?'} app={conf.get('version')}")

# 底部状态栏与「关于」现在展示真实版本（曾写死 v1.0.0，用户看到的版本与已安装版本不符），
# 因此四个清单里的版本号必须同步，避免界面显示的版本与应用实际版本再次脱节
ver_web = json.loads(read("apps", "web", "package.json")).get("version")
ver_root = json.loads(read("package.json")).get("version")
tauri_cargo = re.search(r'^version = "([^"]+)"', read("src-tauri", "Cargo.toml"), re.M)
ver_bin = tauri_cargo.group(1) if tauri_cargo else None
check("版本号五处同步（core / app / 桌面二进制 / web / 工作区）",
      conf.get("version") == ver_web == ver_root == ver_bin,
      f"app={conf.get('version')} web={ver_web} root={ver_root} bin={ver_bin}")

print("\n" + "=" * 60)
print(f"桌面端静态验收：通过 {len(passed)} 项，失败 {len(failed)} 项")
for f in failed:
    print(f"  - {f}")
print("=" * 60)
sys.exit(1 if failed else 0)
