#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
智伴 Mindmate 全量验收：一键运行全部测试套件并汇总（项数为实际统计，避免标签与实现脱节）

用法：
    python scripts/run_all_tests.py            # 推荐：自动拉起临时测试服务（独立数据目录）
    python scripts/run_all_tests.py <base_url> # 指定已运行的服务（必须带 MINDMATE_ALLOW_WIPE=1 才允许清库）
"""

# ⚠️ 数据安全（2026-09-13 的教训）：
# 端到端/提醒/推送三个套件会调用 /data/import 且带 wipe=true（清空 nodes/todos/reports/...）。
# 早期版本要求"先启动应用"再跑本脚本，于是**这些清库操作直接作用在用户的真实数据目录上**，
# 把用户刚录入的待办清掉了。现在本脚本自己拉起一个**独立数据目录**的临时服务：
#   --mode server --headless --data-dir <临时目录> --port <空闲端口>
# 三个套件必须同时满足「MINDMATE_ALLOW_WIPE=1」且「服务是 server 模式（不是用户的桌面实例）」
# 才会执行清库，否则直接拒绝运行。详见 scripts/e2e_test.py 顶部的说明。
import io
import os
import re
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

for s in (sys.stdout, sys.stderr):
    try:
        s.reconfigure(encoding="utf-8")
    except Exception:
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# cargo 通常不在 PATH（本机为用户级安装），显式补上
CARGO_BIN = os.path.join(os.path.expanduser("~"), ".cargo", "bin")
os.environ["PATH"] = CARGO_BIN + os.pathsep + os.environ.get("PATH", "")
CARGO = os.path.join(CARGO_BIN, "cargo.exe") if os.name == "nt" else "cargo"
NPM = "npm.cmd" if os.name == "nt" else "npm"
ARGS = sys.argv[1:]
results = []  # (title, ok, count)


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def extract_count(out: str):
    """提取通过项数：自定义套件读「通过 N 项」；cargo 累加「N passed」"""
    m = re.search(r"通过\s+(\d+)\s+项", out)
    if m:
        return int(m.group(1))
    passed = [int(x) for x in re.findall(r"(\d+) passed", out)]
    return sum(passed) if passed else None


def run(title, cmd, cwd=ROOT, env=None, show=("通过", "test result:", "FAIL", "error", "[FAIL]")):
    print("\n" + "=" * 72)
    print(f"▶ {title}")
    print("=" * 72)
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, env=env)
    out = r.stdout.decode("utf-8", errors="replace")
    for line in out.split("\n"):
        s = line.strip()
        if s and any(k in s for k in show):
            print("  " + s)
    count = extract_count(out)
    ok = r.returncode == 0
    results.append((title, ok, count))
    if not ok and r.stderr:
        print("  STDERR:", r.stderr.decode("utf-8", errors="replace")[-400:])
    return ok


# ── 拉起临时测试服务（独立数据目录，绝不碰用户数据）──
def start_test_server():
    exe = os.path.join(ROOT, "src-tauri", "target", "release", "mindmate.exe" if os.name == "nt" else "mindmate")
    if not os.path.exists(exe):
        print(f"✗ 未找到服务二进制：{exe}")
        print("  请先构建：npm run desktop:build（或 npm run build:core）")
        sys.exit(1)
    data_dir = tempfile.mkdtemp(prefix="mindmate-tests-")
    port = free_port()
    base = f"http://127.0.0.1:{port}"
    print(f"临时测试服务：data-dir={data_dir} port={port}")
    # MINDMATE_AI_KEY="" ：强制"未配置 AI Key"，让套件确定性走本地降级路径。
    # 系统钥匙串是全局的，若不显式清掉，测试会拿用户真实 Key 去打真实上游 API，
    # 上游限流/网络波动就会让断言随机失败（长期存在的"偶发 175/176"即源于此）。
    child_env = dict(os.environ, MINDMATE_AI_KEY="")
    proc = subprocess.Popen(
        [exe, "--mode", "server", "--headless", "--port", str(port), "--data-dir", data_dir],
        cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=child_env,
    )
    for _ in range(60):  # 最多等 30 秒
        try:
            with urllib.request.urlopen(base + "/api/v1/healthz", timeout=2) as r:
                if r.status == 200:
                    return proc, base, data_dir
        except Exception:
            time.sleep(0.5)
    proc.kill()
    print("✗ 临时测试服务启动失败（30 秒内未就绪）")
    sys.exit(1)


server, BASE_URL, TEST_DATA_DIR = start_test_server()
# 三个破坏性套件凭这两个变量才允许清库：一个显式确认，一个确认打的是临时服务
TEST_ENV = dict(os.environ, MINDMATE_ALLOW_WIPE="1", MINDMATE_TEST_DATA_DIR=TEST_DATA_DIR)
SUITE_ARGS = [BASE_URL] if not ARGS else ARGS

try:
    run("Rust 单元测试", [CARGO, "test", "--manifest-path", "core/Cargo.toml"])
    run("端到端接口验收", [sys.executable, "scripts/e2e_test.py"] + SUITE_ARGS, env=TEST_ENV)
    run("提醒系统专项验收（FR-4）", [sys.executable, "scripts/reminder_test.py"] + SUITE_ARGS, env=TEST_ENV)
    run("推送渠道专项验收（FR-4.10）", [sys.executable, "scripts/push_test.py"] + SUITE_ARGS, env=TEST_ENV)
    run("桌面端静态验收（IPC 授权/速记浮窗）", [sys.executable, "scripts/desktop_test.py"])
    run("国际化专项验收（四语/插值）", [sys.executable, "scripts/i18n_test.py"])
    run("国际化消息编译验收（防白屏）", ["node", "scripts/i18n_compile_test.mjs"])
    run("前端生产构建", [NPM, "run", "build"], cwd=os.path.join(ROOT, "apps", "web"))
finally:
    server.kill()
    import shutil
    shutil.rmtree(TEST_DATA_DIR, ignore_errors=True)
    print(f"\n临时测试服务已停止，数据目录已清理：{TEST_DATA_DIR}")

print("\n" + "=" * 72)
print("验收汇总")
print("=" * 72)
total = 0
for title, ok, count in results:
    label = f"{count} 项" if count else "—"
    if count:
        total += count
    print(f"  {'✅ 通过' if ok else '❌ 失败'}  {title:<30s} {label}")
all_ok = all(ok for _, ok, _ in results)
print("=" * 72)
print(f"断言总数：{total} 项 · " + ("全部验收通过 ✅" if all_ok else "存在失败项 ❌"))
sys.exit(0 if all_ok else 1)
