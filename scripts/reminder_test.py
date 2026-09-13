#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
提醒系统专项验收：上下文智能提醒是否按规则真实触发（FR-4 核心差异化）

验证场景（每个场景先建立 SSE 监听，再触发一次提醒相关设置变更以重置计时，
从而保证观测窗口内必然发生一次规则评估，避免竞态）：
  1. 连续未记录（streak=0）  → 关怀提醒 kind=care
  2. 未达标 + 无逾期         → 常规提醒 kind=record（含进度文案）
  3. 当日已达标              → 静默（不打扰）
  4. 存在逾期待办            → 加压提醒 kind=overdue（含标题与数量）
  5. 当前时间在生效时段外    → 静默
  6. 勿扰时段内              → 静默

用法：python scripts/reminder_test.py [base_url]
"""
import json
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import date, timedelta

for stream in (sys.stdout, sys.stderr):
    try:
        stream.reconfigure(encoding="utf-8")
    except Exception:
        pass

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:17801") + "/api/v1"
TODAY = date.today().isoformat()
YESTERDAY = (date.today() - timedelta(days=1)).isoformat()
TOKEN = None
passed, failed = [], []


# ───────────────────────── HTTP ─────────────────────────

def call(method, path, body=None):
    if "?" in path:
        b, qs = path.split("?", 1)
        parts = []
        for kv in qs.split("&"):
            if "=" in kv:
                k, v = kv.split("=", 1)
                parts.append(f"{k}={urllib.parse.quote(v)}")
            else:
                parts.append(kv)
        path = b + "?" + "&".join(parts)
    data = json.dumps(body, ensure_ascii=False).encode("utf-8") if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if TOKEN:
        req.add_header("Authorization", "Bearer " + TOKEN)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            return json.loads(r.read().decode("utf-8")), r.status
    except urllib.error.HTTPError as e:
        return json.loads(e.read().decode("utf-8")), e.code


def set_settings(values):
    return call("PUT", "/settings", {"values": values})


def check(name, cond, detail=""):
    (passed if cond else failed).append(name)
    print(f"  [{'PASS' if cond else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))


def minutes_ago(n):
    return time.strftime("%H:%M", time.localtime(time.time() - n * 60))


def minutes_ahead(n):
    return time.strftime("%H:%M", time.localtime(time.time() + n * 60))


# ───────────────────────── SSE 监听 ─────────────────────────

def listen_reminders(seconds):
    """订阅 SSE 收集 reminder.triggered（超时须大于服务端 15s 心跳，避免连接被读超时废弃）"""
    events, stop = [], threading.Event()

    def run():
        try:
            req = urllib.request.Request(
                BASE + "/stream/events" + (f"?token={TOKEN}" if TOKEN else "")
            )
            with urllib.request.urlopen(req, timeout=25) as resp:
                name = ""
                deadline = time.time() + seconds
                while time.time() < deadline and not stop.is_set():
                    try:
                        raw = resp.readline()
                    except Exception:
                        if time.time() >= deadline:
                            break
                        continue
                    if not raw:
                        break
                    line = raw.decode("utf-8", errors="ignore").strip()
                    if line.startswith("event:"):
                        name = line[6:].strip()
                    elif line.startswith("data:") and name == "reminder.triggered":
                        try:
                            # 广播体结构为 {kind, payload, at}，提醒详情在 payload 内
                            body = json.loads(line[5:].strip())
                            events.append(body.get("payload", body))
                        except Exception:
                            pass
        except Exception:
            pass

    t = threading.Thread(target=run, daemon=True)
    t.start()
    return events, stop, t


def observe(seconds, trigger_settings, warmup=1.5):
    """先建立监听 → 再触发设置变更（服务端立即重新评估）→ 收集窗口内提醒"""
    events, stop, th = listen_reminders(seconds)
    time.sleep(warmup)
    if trigger_settings:
        set_settings(trigger_settings)
    th.join(timeout=seconds + 12)
    stop.set()
    return events


# ───────────────────────── 登录 ─────────────────────────

r, _ = call("GET", "/auth/status")
status = r.get("data") or {}
needs_auth = status.get("requiresLogin")
if needs_auth:
    if not status.get("hasPassword"):
        call("POST", "/auth/set-password", {"password": "mindmate-e2e"})
    r, _ = call("POST", "/auth/login", {"password": "mindmate-e2e"})
    TOKEN = (r.get("data") or {}).get("token")
print(f"运行模式：{status.get('mode')} · 已认证：{bool(TOKEN) or not needs_auth}")

# ───────────────────────── 准备 ─────────────────────────

print("\n=== 准备：清空数据并设置覆盖当前时刻的提醒窗口 ===")
call("POST", "/data/import", {"payload": {"nodes": [], "todos": []}, "wipe": True})
BASE_SETTINGS = {
    "daily_goal": "4", "daily_goal_enabled": "1",
    "remind_freq_minutes": "60", "remind_enabled": "1",
    "remind_window_start": minutes_ago(30), "remind_window_end": minutes_ahead(90),
    "dnd_rules": "[]", "todo_remind_enabled": "0",
    "smart_brief_enabled": "0", "goodnight_enabled": "0",
}
set_settings(BASE_SETTINGS)
print(f"  提醒生效时段：{BASE_SETTINGS['remind_window_start']} – {BASE_SETTINGS['remind_window_end']}（覆盖当前时刻）")

# ───────────────────────── 场景 1：连续未记录 → 关怀 ─────────────────────────

print("\n=== 场景 1：连续未记录（streak=0）→ 关怀提醒 ===")
ev = observe(24, BASE_SETTINGS)
care = [e for e in ev if e.get("kind") == "care"]
check("触发关怀提醒", len(care) >= 1, f"收到 {[e.get('kind') for e in ev]}")
if care:
    check("关怀文案温和（含关切语气）", "还好吗" in care[0].get("body", ""), care[0].get("body", ""))
    check("点击行为=速记浮窗", care[0].get("action") == "quick_entry")

# ───────────────────────── 场景 2：未达标 + 无逾期 → 常规提醒 ─────────────────────────

print("\n=== 场景 2：未达标 + 无逾期 → 常规记录提醒 ===")
# 补一条昨天的记录使连续记录 >= 1（否则按设计走关怀分支）
call("POST", "/nodes", {"content": "昨天的记录（建立连续记录）", "date": YESTERDAY})
ev = observe(24, {"remind_freq_minutes": "60"})
rec = [e for e in ev if e.get("kind") == "record"]
check("触发记录提醒", len(rec) >= 1, f"收到 {[e.get('kind') for e in ev]}")
if rec:
    check("提醒文案含进度（已录 n/目标 m）", "/" in rec[0].get("body", ""), rec[0].get("body", ""))
    check("提醒标题符合文案规范", rec[0].get("title") == "该记录一下了", rec[0].get("title", ""))

# ───────────────────────── 场景 3：已达标 → 静默 ─────────────────────────

print("\n=== 场景 3：当日记录已达标 → 静默不打扰 ===")
set_settings({"daily_goal": "1", "daily_goal_enabled": "1"})
for i in range(2):
    call("POST", "/nodes", {"content": f"达标记录 {i + 1}", "date": TODAY})
r, _ = call("GET", f"/stats/daily?date={TODAY}")
st = r.get("data") or {}
print(f"  当前已录 {st.get('nodeCount')} 条 / 目标 {st.get('dailyGoal')} 条")
ev = observe(24, {"daily_goal": "1"})
hit = [e for e in ev if e.get("kind") in ("record", "overdue", "care")]
check("达标后静默（窗口内零提醒）", len(hit) == 0, f"收到 {[e.get('kind') for e in ev]}")

# ───────────────────────── 场景 4：有逾期 → 加压提醒 ─────────────────────────

print("\n=== 场景 4：存在逾期待办 → 加压提醒 ===")
set_settings({"daily_goal": "99"})  # 避免被达标静默规则拦截
call("POST", "/todos", {"title": "修复登录 bug", "dueDate": YESTERDAY, "priority": "高"})
ev = observe(24, {"remind_freq_minutes": "60"})
od = [e for e in ev if e.get("kind") == "overdue"]
check("触发逾期加压提醒", len(od) >= 1, f"收到 {[e.get('kind') for e in ev]}")
if od:
    check("文案包含逾期待办标题", "修复登录 bug" in od[0].get("body", ""), od[0].get("body", ""))
    check("标题含逾期数量", "逾期" in od[0].get("title", ""), od[0].get("title", ""))

# ───────────────────────── 场景 5：时段外 → 静默 ─────────────────────────

print("\n=== 场景 5：当前时间在生效时段之外 → 静默 ===")
ev = observe(24, {"remind_window_start": "00:01", "remind_window_end": "00:02"})
check("时段外静默（零提醒）", len(ev) == 0, f"收到 {[e.get('kind') for e in ev]}")

# ───────────────────────── 场景 6：勿扰 → 静默 ─────────────────────────

print("\n=== 场景 6：勿扰时段内 → 完全静默 ===")
ev = observe(24, {
    "remind_window_start": minutes_ago(30), "remind_window_end": minutes_ahead(90),
    "dnd_rules": json.dumps([{"start": minutes_ago(30), "end": minutes_ahead(30), "date": ""}]),
})
check("勿扰时段静默（零提醒）", len(ev) == 0, f"收到 {[e.get('kind') for e in ev]}")

# ───────────────────────── 恢复默认 ─────────────────────────

set_settings({
    "dnd_rules": "[]", "remind_window_start": "09:00", "remind_window_end": "21:00",
    "daily_goal": "4", "remind_freq_minutes": "60", "todo_remind_enabled": "1",
    "smart_brief_enabled": "1", "goodnight_enabled": "1", "daily_goal_enabled": "1",
})

print("\n" + "=" * 60)
print(f"提醒系统验收：通过 {len(passed)} 项，失败 {len(failed)} 项")
for f in failed:
    print(f"  - {f}")
print("=" * 60)
sys.exit(1 if failed else 0)
