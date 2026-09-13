#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
智伴 Mindmate 全量验收：一键运行全部测试套件并汇总（项数为实际统计，避免标签与实现脱节）

用法：
    python scripts/run_all_tests.py [base_url]
    （需先启动服务：mindmate --mode server --headless 或桌面端）
"""
import os
import re
import subprocess
import sys

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


def extract_count(out: str):
    """提取通过项数：自定义套件读「通过 N 项」；cargo 累加「N passed」"""
    m = re.search(r"通过\s+(\d+)\s+项", out)
    if m:
        return int(m.group(1))
    passed = [int(x) for x in re.findall(r"(\d+) passed", out)]
    return sum(passed) if passed else None


def run(title, cmd, cwd=ROOT):
    print("\n" + "=" * 72)
    print(f"▶ {title}")
    print("=" * 72)
    r = subprocess.run(cmd, cwd=cwd, capture_output=True)
    out = r.stdout.decode("utf-8", errors="replace")
    for line in out.split("\n"):
        s = line.strip()
        if s and any(k in s for k in ("通过", "test result:", "FAIL", "error", "[FAIL]")):
            print("  " + s)
    count = extract_count(out)
    ok = r.returncode == 0
    results.append((title, ok, count))
    if not ok and r.stderr:
        print("  STDERR:", r.stderr.decode("utf-8", errors="replace")[-400:])
    return ok


run("Rust 单元测试", [CARGO, "test", "--manifest-path", "core/Cargo.toml"])
run("端到端接口验收", [sys.executable, "scripts/e2e_test.py"] + ARGS)
run("提醒系统专项验收（FR-4）", [sys.executable, "scripts/reminder_test.py"] + ARGS)
run("推送渠道专项验收（FR-4.10）", [sys.executable, "scripts/push_test.py"] + ARGS)
run("桌面端静态验收（IPC 授权/速记浮窗）", [sys.executable, "scripts/desktop_test.py"])
run("前端生产构建", [NPM, "run", "build"], cwd=os.path.join(ROOT, "apps", "web"))

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
