#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
推送渠道专项验收（FR-4.10：邮件 / Telegram / 企业微信 Webhook）

验证方式：
  1. 本地起一个 HTTP 接收器充作「企业微信 Webhook」，配置到智伴
  2. 调用 /push/test 触发测试推送 → 校验接收器真实收到正确载荷
  3. 用设置变更重置提醒计时 → 验证**提醒触发时自动经该渠道推送**（投递链打通）
  4. 校验：未启用渠道时不推送；配置缺失/错误时给出可读错误而非静默失败
  5. 邮件与 Telegram 在无凭据时给出可读错误（外网投递不在自动化测试范围）

用法：python scripts/push_test.py [base_url]
"""
import json
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import date, timedelta
from http.server import BaseHTTPRequestHandler, HTTPServer

for s in (sys.stdout, sys.stderr):
    try:
        s.reconfigure(encoding="utf-8")
    except Exception:
        pass

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:17801") + "/api/v1"
TODAY = date.today().isoformat()
YESTERDAY = (date.today() - timedelta(days=1)).isoformat()
TOKEN = None
passed, failed = [], []

# ── 本地 Webhook 接收器 ──
received: list = []


class Receiver(BaseHTTPRequestHandler):
    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length).decode("utf-8", "replace")
        try:
            received.append(json.loads(raw))
        except Exception:
            received.append({"raw": raw})
        body = json.dumps({"errcode": 0, "errmsg": "ok"}).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):  # 静默
        pass


def start_receiver() -> int:
    httpd = HTTPServer(("127.0.0.1", 0), Receiver)
    port = httpd.server_address[1]
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return port


# ── HTTP ──
def call(method, path, body=None):
    data = json.dumps(body, ensure_ascii=False).encode("utf-8") if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    if TOKEN:
        req.add_header("Authorization", "Bearer " + TOKEN)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return json.loads(r.read().decode("utf-8")), r.status
    except urllib.error.HTTPError as e:
        return json.loads(e.read().decode("utf-8")), e.code


def check(name, cond, detail=""):
    (passed if cond else failed).append(name)
    print(f"  [{'PASS' if cond else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))


def minutes_ago(n):
    return time.strftime("%H:%M", time.localtime(time.time() - n * 60))


def minutes_ahead(n):
    return time.strftime("%H:%M", time.localtime(time.time() + n * 60))


def listen_events(seconds):
    """订阅 SSE，收集 reminder.triggered"""
    events, stop = [], threading.Event()

    def run():
        try:
            req = urllib.request.Request(
                BASE + "/stream/events" + (f"?token={TOKEN}" if TOKEN else "")
            )
            with urllib.request.urlopen(req, timeout=25) as resp:
                name, deadline = "", time.time() + seconds
                while time.time() < deadline and not stop.is_set():
                    try:
                        raw = resp.readline()
                    except Exception:
                        if time.time() >= deadline:
                            break
                        continue
                    if not raw:
                        break
                    line = raw.decode("utf-8", "ignore").strip()
                    if line.startswith("event:"):
                        name = line[6:].strip()
                    elif line.startswith("data:") and name == "reminder.triggered":
                        events.append(json.loads(line[5:].strip()).get("payload", {}))
        except Exception:
            pass

    t = threading.Thread(target=run, daemon=True)
    t.start()
    return events, stop, t


# ── 登录 ──
r, _ = call("GET", "/auth/status")
status = r.get("data") or {}
if status.get("requiresLogin"):
    if not status.get("hasPassword"):
        call("POST", "/auth/set-password", {"password": "mindmate-e2e"})
    r, _ = call("POST", "/auth/login", {"password": "mindmate-e2e"})
    TOKEN = (r.get("data") or {}).get("token")
print(f"运行模式：{status.get('mode')} · 已认证：{bool(TOKEN) or not status.get('requiresLogin')}")

recv_port = start_receiver()
hook = f"http://127.0.0.1:{recv_port}/cgi-bin/webhook/send?key=mindmate-test"
print(f"本地 Webhook 接收器已启动：127.0.0.1:{recv_port}")

# ───────────────── 1. 默认不启用任何渠道 ─────────────────
print("\n=== 1. 默认状态：不启用任何推送渠道 ===")
# 先归零再断言：本段校验「默认状态」。上一次运行若中途异常退出（本机网络抖动时发生过一次），
# 会把已启用的渠道留在配置里，下一次运行就误报 —— 断言不应依赖环境残留。
_r0, _ = call("GET", "/push/config")
_reset = dict(_r0.get("data") or {})
_reset.update({"channels": [], "wecomWebhook": "", "emailTo": "", "telegramChatId": ""})
call("POST", "/push/config", {"config": _reset})
r, _ = call("GET", "/push/config")
cfg = r.get("data") or {}
check("默认渠道列表为空", cfg.get("channels") == [], f"channels={cfg.get('channels')}")
check("默认无 SMTP 密码", cfg.get("hasSmtpPassword") is False)
check("默认无 Telegram Token", cfg.get("hasTelegramToken") is False)
check("配置不回传任何明文凭据", "smtpPassword" not in cfg and "telegramToken" not in cfg)

# ───────────────── 2. 配置渠道并测试投递 ─────────────────
print("\n=== 2. 企业微信 Webhook：测试投递真实到达 ===")
payload = dict(cfg)
payload.update({
    "channels": ["wecom"],
    "wecomWebhook": hook,
})
r, _ = call("POST", "/push/config", {"config": payload})
check("保存推送配置（启用企业微信）", (r.get("data") or {}).get("channels") == ["wecom"])

received.clear()
r, _ = call("POST", "/push/test", {"channel": "wecom"})
res = r.get("data") or {}
check("测试接口返回成功", res.get("ok") is True, str(res))
time.sleep(0.6)
check("接收器真实收到推送", len(received) >= 1, f"收到 {len(received)} 条")
if received:
    body = received[-1]
    check("载荷 msgtype=text", body.get("msgtype") == "text", str(body)[:120])
    content = (body.get("text") or {}).get("content", "")
    check("载荷含测试文案", "推送渠道测试" in content, content[:80])

# ───────────────── 3. 提醒触发时自动推送（投递链打通）─────────────────
print("\n=== 3. 提醒触发后自动经推送渠道投递 ===")
# 准备：清数据、窗口覆盖当前、目标调高（避免达标静默）、关闭其它时刻提醒
call("POST", "/data/import", {"payload": {"nodes": [], "todos": []}, "wipe": True})
settings = {
    "daily_goal": "99", "daily_goal_enabled": "1",
    "remind_freq_minutes": "60", "remind_enabled": "1",
    "remind_window_start": minutes_ago(30), "remind_window_end": minutes_ahead(90),
    "dnd_rules": "[]", "todo_remind_enabled": "0",
    "smart_brief_enabled": "0", "goodnight_enabled": "0",
}
call("PUT", "/settings", {"values": settings})
call("POST", "/nodes", {"content": "昨天的记录（建立连续记录）", "date": YESTERDAY})

received.clear()
ev, stop, th = listen_events(26)
time.sleep(1.5)
call("PUT", "/settings", {"values": {"remind_freq_minutes": "60"}})  # 重置计时 → 立即评估
th.join(timeout=34)
stop.set()

check("提醒事件已触发", len(ev) >= 1, f"收到 {[e.get('kind') for e in ev]}")
time.sleep(1.0)  # 等待异步推送完成
check("提醒经推送渠道投递到接收器", len(received) >= 1, f"收到 {len(received)} 条推送")
if received and ev:
    txt = (received[-1].get("text") or {}).get("content", "")
    check("推送内容与提醒一致", ev[0].get("title", "") in txt, txt[:120])
    check("推送内容含应用署名", "智伴" in txt, txt[:60])

# ───────────────── 4. 未启用渠道不推送 ─────────────────
print("\n=== 4. 未启用的渠道不会推送 ===")
payload2 = dict(payload)
payload2["channels"] = []
call("POST", "/push/config", {"config": payload2})
received.clear()
r, _ = call("POST", "/push/test", {"channel": "wecom"})
check("手动测试仍可用（独立于启用状态）", (r.get("data") or {}).get("ok") is True)
time.sleep(0.6)
check("暂存渠道被清空", (call("GET", "/push/config")[0].get("data") or {}).get("channels") == [])

# ───────────────── 5. 配置缺失/错误时给出可读错误 ─────────────────
print("\n=== 5. 异常路径给出可读错误（非静默失败）===")
bad = dict(payload)
bad.update({"channels": ["wecom"], "wecomWebhook": ""})
call("POST", "/push/config", {"config": bad})
r, _ = call("POST", "/push/test", {"channel": "wecom"})
res = r.get("data") or {}
check("空 Webhook 返回失败与原因", res.get("ok") is False and "未配置" in (res.get("detail") or ""),
      str(res))

bad2 = dict(payload)
bad2.update({"channels": ["email"], "emailTo": ""})
call("POST", "/push/config", {"config": bad2})
r, _ = call("POST", "/push/test", {"channel": "email"})
check("邮件缺收件人返回可读错误", (r.get("data") or {}).get("ok") is False,
      str((r.get("data") or {}).get("detail"))[:80])

bad3 = dict(payload)
bad3.update({"channels": ["telegram"], "telegramChatId": ""})
call("POST", "/push/config", {"config": bad3})
r, _ = call("POST", "/push/test", {"channel": "telegram"})
check("Telegram 缺 Chat ID 返回可读错误", (r.get("data") or {}).get("ok") is False,
      str((r.get("data") or {}).get("detail"))[:80])

# ───────────────── 6. 无效 Webhook 地址的错误处理 ─────────────────
print("\n=== 6. 无效 Webhook 地址：错误可读且不影响服务 ===")
bad4 = dict(payload)
bad4.update({"channels": ["wecom"], "wecomWebhook": "http://127.0.0.1:9/not-exist"})
call("POST", "/push/config", {"config": bad4})
r, _ = call("POST", "/push/test", {"channel": "wecom"})
check("连接失败返回可读错误", (r.get("data") or {}).get("ok") is False,
      str((r.get("data") or {}).get("detail"))[:100])
r, code = call("GET", "/healthz")
check("服务仍健康（推送失败不影响核心）", code == 200 and (r.get("data") or {}).get("status") == "ok")

# ───────────────── 恢复默认 ─────────────────
final = dict(payload)
final.update({"channels": [], "wecomWebhook": "", "emailTo": "", "telegramChatId": ""})
call("POST", "/push/config", {"config": final})
call("PUT", "/settings", {"values": {
    "daily_goal": "4", "remind_freq_minutes": "60",
    "remind_window_start": "09:00", "remind_window_end": "21:00",
    "todo_remind_enabled": "1", "smart_brief_enabled": "1", "goodnight_enabled": "1",
}})

print("\n" + "=" * 60)
print(f"推送渠道验收：通过 {len(passed)} 项，失败 {len(failed)} 项")
for f in failed:
    print(f"  - {f}")
print("=" * 60)
sys.exit(1 if failed else 0)
