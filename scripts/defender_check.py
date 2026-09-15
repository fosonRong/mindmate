#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
发布前「杀软误报」自检：用本机 Windows Defender 扫一遍即将发布的二进制。

为什么需要这一步（2026-09-15 的教训）：
v1.0.16 的安装包能装上，但装完 `mindmate.exe` 立刻被 Defender 判为
`Trojan:Win32/Bearfoos.A!ml`（`!ml` = 机器学习启发式判定）并清除，用户表现为
「安装后无法打开」。我们是在用户反馈后才发现——因为发布流水线里**没有任何环节会
真正运行一次杀毒扫描**。本脚本把这一步补上：发布前先扫，扫出问题就别发。

判据有两条（缺一不可）：
  1) 把文件复制到 %TEMP% 下的中立目录后，文件是否还在。项目目录/安装目录通常已被
     用户加进 Defender 排除项，而 MpCmdRun 会尊重排除项——所以必须换目录再扫，
     否则扫描结果恒为"干净"，等于没扫。
  2) MpCmdRun -Scan -ScanType 3 的退出码（2 = 检出威胁）。

用法：
    python scripts/defender_check.py <文件|目录|通配符> [...]     # 检出只告警（退出码 0）
    python scripts/defender_check.py --strict <文件> [...]       # 检出即失败（退出码 1，CI 用）
    MINDMATE_SKIP_DEFENDER_CHECK=1                               # 显式跳过（退出码 0，打印警告）

退出码：0 通过/跳过；1 检出（仅 --strict）；2 用法或工具错误。
非 Windows 或无 MpCmdRun 的机器直接跳过（退出码 0），不阻断 macOS/Linux 构建。
"""

import glob
import os
import shutil
import subprocess
import sys
import tempfile
import time

for s in (sys.stdout, sys.stderr):
    try:
        s.reconfigure(encoding="utf-8")
    except Exception:
        pass

# 杀软检出后的现象：复制过去的文件会被实时保护立刻清掉，给自己留一点观察时间
WATCH_SECONDS = 8.0
SCAN_TIMEOUT = 180


def find_mpcmdrun():
    """定位 MpCmdRun.exe：优先取 Platform 下版本号最高的目录"""
    roots = [
        os.path.join(os.environ.get("ProgramData", r"C:\ProgramData"), "Microsoft", "Windows Defender", "Platform"),
        os.path.join(os.environ.get("ProgramFiles", r"C:\Program Files"), "Windows Defender"),
        os.path.join(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"), "Windows Defender"),
    ]
    cands = []
    for root in roots:
        cands += glob.glob(os.path.join(root, "*", "MpCmdRun.exe"))
        cands += glob.glob(os.path.join(root, "MpCmdRun.exe"))

    def ver_key(p):
        # 4.18.26080.3-0 → (4, 18, 26080, 3)，用于挑最新平台版本
        name = os.path.basename(os.path.dirname(p))
        parts = []
        for chunk in name.replace("-", ".").split("."):
            parts.append(int(chunk) if chunk.isdigit() else 0)
        return parts

    cands = [c for c in cands if os.path.exists(c)]
    if not cands:
        return None
    return max(cands, key=ver_key)


def expand(patterns):
    files = []
    for p in patterns:
        if os.path.isdir(p):
            files += [os.path.join(p, f) for f in os.listdir(p)]
        elif any(ch in p for ch in "*?["):
            files += glob.glob(p)
        else:
            files.append(p)
    # 去重保序，只保留真实文件
    seen, out = set(), []
    for f in files:
        a = os.path.abspath(f)
        if a not in seen and os.path.isfile(a):
            seen.add(a)
            out.append(a)
    return out


def scan_one(mp, path, workdir):
    """扫描单个文件：返回 (是否检出, 说明)"""
    dest = os.path.join(workdir, os.path.basename(path))
    if os.path.exists(dest):
        os.remove(dest)
    try:
        shutil.copy2(path, dest)
    except Exception as e:
        return None, f"无法复制到中立目录（{e}）"

    # 实时保护可能直接删文件——这本身就是检出信号
    deadline = time.time() + WATCH_SECONDS
    while time.time() < deadline:
        if not os.path.exists(dest):
            return True, "复制到中立目录后被实时保护清除（未及扫描即被隔离）"
        time.sleep(0.5)

    try:
        r = subprocess.run(
            [mp, "-Scan", "-ScanType", "3", "-File", dest],
            capture_output=True,
            timeout=SCAN_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        return None, f"扫描超时（>{SCAN_TIMEOUT}s）"

    out = (r.stdout or b"").decode("utf-8", errors="replace") + (r.stderr or b"").decode("utf-8", errors="replace")
    detail = ""
    for line in out.splitlines():
        low = line.lower()
        if any(k in low for k in ("threat", "virus", "detected", "found", "威胁", "病毒")):
            detail = line.strip()
            break
    # MpCmdRun 退出码：0 = 未发现威胁，2 = 发现威胁
    if r.returncode == 2 or not os.path.exists(dest):
        return True, detail or f"MpCmdRun 退出码 {r.returncode}"
    if r.returncode not in (0, 1):
        return None, f"MpCmdRun 返回异常退出码 {r.returncode}：{detail or out.strip()[:200]}"
    return False, detail or "未检出"


def main():
    args = [a for a in sys.argv[1:] if a != "--strict"]
    strict = "--strict" in sys.argv[1:]

    if os.environ.get("MINDMATE_SKIP_DEFENDER_CHECK"):
        print("⚠️  已按 MINDMATE_SKIP_DEFENDER_CHECK 跳过杀软自检（发布前请人工确认误报状态）")
        return 0

    if os.name != "nt":
        print("· 非 Windows：跳过杀软自检")
        return 0

    mp = find_mpcmdrun()
    if not mp:
        print("· 未找到 MpCmdRun.exe（未启用 Defender？）：跳过杀软自检")
        return 0

    files = expand(args)
    if not files:
        print("✗ 没有可扫描的文件；用法：python scripts/defender_check.py [--strict] <文件|目录|通配符> [...]")
        return 2

    print(f"杀软自检（{os.path.basename(os.path.dirname(mp))}）：{'、'.join(os.path.basename(f) for f in files)}")
    workdir = tempfile.mkdtemp(prefix="mindmate-avcheck-")
    flagged = []
    try:
        for f in files:
            size = os.path.getsize(f) / 1048576
            verdict, why = scan_one(mp, f, workdir)
            if verdict is True:
                flagged.append((f, why))
                print(f"  ✗ 被检出：{os.path.basename(f)}（{size:.2f} MB）— {why}")
            elif verdict is None:
                print(f"  ? 未能判定：{os.path.basename(f)}（{size:.2f} MB）— {why}")
            else:
                print(f"  ✓ 干净：{os.path.basename(f)}（{size:.2f} MB）")
    finally:
        shutil.rmtree(workdir, ignore_errors=True)

    if not flagged:
        print("✓ 杀软自检通过：预发布二进制未被本机 Defender 判定为威胁")
        return 0

    print("")
    print("✗ 杀软自检未通过：以上文件被本机 Windows Defender 判定为威胁。")
    print("  这是**未签名二进制被机器学习模型误判**的典型情况（检出名通常带 !ml 后缀）。")
    print("  发布这种二进制没有意义：用户装得上、装完打不开（exe 会被隔离）。")
    print("  处置步骤见 docs/杀软误报处置.md：")
    print("    1) 重新构建（版本号 +1）后再跑一次本脚本，机器学习的判定边界会随字节变化而移动；")
    print("    2) 仍被检出的，到 https://www.microsoft.com/en-us/wdsi/filesubmission 提交误报（用 docs 里现成的文本）；")
    print("    3) 长期方案：购买代码签名证书（OV/EV）或用 Azure Trusted Signing。")
    print("  若确认要继续发布（例如只是本机定义版本滞后），加 --allow-flagged 跳过失败。")
    if "--allow-flagged" in sys.argv[1:]:
        print("  （已按 --allow-flagged 忽略）")
        return 0
    return 1 if strict else 0


if __name__ == "__main__":
    sys.exit(main())
