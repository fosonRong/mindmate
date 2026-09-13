#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
国际化专项验收（FR-1 国际化 / T1.1–T1.3）

为什么必须有这个套件：i18n 的失效往往是"静默"的——
  · 词条缺失 → 界面回退中文（看起来只是"没翻译"，不报错）
  · **zh-CN 缺 key → vue-i18n 原样返回 key 且不做插值，界面出现 {a} 字面量**（真机踩过）
  · 译文漏掉 {a}/{b} → 界面少显示数据，同样不报错
  · 模板里残留硬编码中文 → 切换语言时那部分不跟着变
因此逐条断言：四语覆盖率、占位符一致性、模板零残留、脚本区无硬编码显示文案。

用法：python scripts/i18n_test.py
"""
import io
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
SRC = os.path.join(ROOT, "apps", "web", "src")
MSG_DIR = os.path.join(SRC, "i18n", "messages")
LANGS = ["zh-CN", "en-US", "ja-JP", "ko-KR"]
passed, failed = [], []


def check(name, cond, detail=""):
    (passed if cond else failed).append(name)
    print(f"  [{'PASS' if cond else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))


def read(p):
    return io.open(p, encoding="utf-8").read()


# ── 复用词条工具的实现，避免两套逻辑分叉 ──
sys.path.insert(0, os.path.join(ROOT, "scripts"))
import i18n_catalog as cat  # noqa: E402

keys = cat.collect_keys()
print(f"\n代码中使用的中文文案 key：{len(keys)} 条\n")

# ── 1. 四语覆盖率 ──
print("1. 四语词条覆盖率")
for lang in LANGS:
    c = cat.read_catalog(lang)
    missing = [k for k in keys if k not in c]
    check(f"{lang} 覆盖全部 {len(keys)} 条文案", not missing,
          f"缺 {len(missing)} 条：{missing[:2]}" if missing else "100%")

# ── 2. 插值占位符一致性（漏掉会让界面少显示数据）──
print("\n2. 插值占位符一致性")
zh = cat.read_catalog("zh-CN")
for lang in LANGS[1:]:
    c = cat.read_catalog(lang)
    bad = []
    for k in keys:
        v = c.get(k)
        if v is None:
            continue
        if set(cat.PLACEHOLDER.findall(k)) != set(cat.PLACEHOLDER.findall(v)):
            bad.append(k)
    check(f"{lang} 全部条目占位符与原文一致", not bad, f"{len(bad)} 条不一致：{bad[:2]}" if bad else "一致")

# ── 3. zh-CN 必须显式收录（否则插值失效，界面出现 {a}）──
print("\n3. zh-CN 基线与插值")
only_interp = [k for k in keys if cat.PLACEHOLDER.search(k)]
check("带占位符的文案已在 zh-CN 显式收录（保证插值生效）",
      all(k in zh for k in only_interp), f"带占位符文案 {len(only_interp)} 条")
check("zh-CN 每条的值等于 key（原文即 key 的约定）",
      all(zh.get(k) == k for k in keys if k in zh))

# ── 3.5 动态 key（导航标题/模式名等由变量传入，静态抽取看不到）──
print("\n3.5 动态 key（导航与模式名）")
DYNAMIC_KEYS = ["今日", "周", "月", "待办", "智伴", "设置", "本地模式", "局域网模式", "服务器模式",
                "周一", "周二", "周三", "周四", "周五", "周六", "周日"]
for lang in LANGS:
    c = cat.read_catalog(lang)
    miss = [k for k in DYNAMIC_KEYS if k not in c]
    check(f"{lang} 覆盖动态 key（导航/模式名）", not miss, f"缺 {miss}" if miss else "完整")

# ── 4. 模板不得残留硬编码中文 ──
print("\n4. 模板文案抽取完整性")
r = subprocess.run([sys.executable, os.path.join(ROOT, "scripts", "i18n_tool.py"), "--check"],
                   capture_output=True, cwd=ROOT)
out = r.stdout.decode("utf-8", "replace")
check("模板中无未抽取的中文（切换语言时会跟着变）", r.returncode == 0, out.strip().splitlines()[-1] if out.strip() else "")

# ── 5. 脚本区 toast 文案已接入 i18n ──
print("\n5. 脚本区显示文案")
bad_toast = []
for base, _d, files in os.walk(SRC):
    for f in files:
        if not f.endswith((".vue", ".ts")):
            continue
        p = os.path.join(base, f)
        for i, line in enumerate(read(p).splitlines(), 1):
            if "t(" in line:
                continue  # 已走 t()，无需再看
            # 单引号字面量
            m = re.search(r"\.toast\(\s*'(?:success|error|info|warning)'\s*,\s*'[^']*[\u4e00-\u9fff]", line)
            # 模板字符串（曾用这种方式绕过 i18n，英文界面会露出中文）
            m2 = re.search(r"\.toast\(\s*'(?:success|error|info|warning)'\s*,\s*`[^`]*[\u4e00-\u9fff]", line)
            if m or m2:
                bad_toast.append(f"{os.path.relpath(p, ROOT)}:{i}")
check("toast 文案均已走 t()（不再硬编码中文）", not bad_toast, ", ".join(bad_toast[:3]))

# ── 6. i18n 运行时接线 ──
print("\n6. 运行时接线")
idx = read(os.path.join(SRC, "i18n", "index.ts"))
main = read(os.path.join(SRC, "main.ts"))
check("i18n 已在 main.ts 注册", "app.use(i18n)" in main)
check("四语词条全部注册进 createI18n", all(l in idx for l in LANGS))
check("默认跟随系统语言", "systemLocale" in idx and "navigator" in idx)
check("语言选择持久化到 localStorage 与服务端 settings", "LOCALE_STORAGE_KEY" in idx and "ui_locale" in idx)

# ── 7. 设置页有语言切换入口 ──
settings = read(os.path.join(SRC, "views", "Settings.vue"))
check("设置页提供界面语言选择（跟随系统 + 四语）",
      "界面语言" in settings and "localeOptions" in settings and "changeLocale" in settings)

# ── 8. 白屏防护（真机踩过：词条语法错误会让整块界面空白）──
# 词条语法本身由 scripts/i18n_compile_test.mjs 逐条编译校验；这里只查"万一还是出错"的兜底链路
print("\n8. 白屏防护")
check("模板 $t 已换成容错版本（解析失败只回退原文，不再白屏）",
      "globalProperties.$t = safeT" in main and "export function safeT" in idx)
check("脚本区 t() 也走容错实现", "safeT(key, named)" in idx or "return named ? safeT" in idx)
safe_view = os.path.join(SRC, "components", "SafeView.vue")
check("存在页面级错误边界组件 SafeView", os.path.exists(safe_view))
check("路由内容已包在错误边界内（异常时渲染提示卡片而非空白）",
      "SafeView" in read(os.path.join(SRC, "App.vue")) and "onErrorCaptured" in (read(safe_view) if os.path.exists(safe_view) else ""))

print("\n" + "=" * 60)
print(f"国际化专项验收：通过 {len(passed)} 项，失败 {len(failed)} 项")
for f in failed:
    print(f"  - {f}")
print("=" * 60)
sys.exit(1 if failed else 0)
