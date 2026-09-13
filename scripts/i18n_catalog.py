#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
四语词条表工具：生成 zh-CN 基线表、校验各语言覆盖率与占位符一致性

为什么需要它：
  1) 「中文原文即 key」方案下，若 zh-CN 词条表为空，vue-i18n 找不到消息时会**原样返回 key**，
     且**不做参数插值** —— 界面会出现 `{a}` 这样的字面量（真机踩到过）。
     因此 zh-CN 必须显式收录全部 key（值等于 key 本身），插值才生效；
     同时 en/ja/ko 未翻译的条目会回退到 zh-CN 的条目，从而**带插值地显示中文**，不会破相。
  2) 翻译最容易犯的错是漏掉占位符（`{a}`/`{b}`），导致界面少显示数据。
     本工具按 key 比对各语言占位符集合，数量或名称不一致即报错。

用法：
    python scripts/i18n_catalog.py --emit-zh    # 生成/更新 zh-CN 基线表（自动收录全部 key）
    python scripts/i18n_catalog.py --check      # 校验覆盖率与占位符一致性
"""
import io
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
SRC = os.path.join(ROOT, "apps", "web", "src")
MSG_DIR = os.path.join(SRC, "i18n", "messages")
LANGS = ["zh-CN", "en-US", "ja-JP", "ko-KR"]
KEY_USE = re.compile(r"(?<![.\w])\$?t\(\s*'((?:[^'\\]|\\.)*)'")
PLACEHOLDER = re.compile(r"\{([a-z])\}")


def collect_keys():
    keys = set()
    for base, _d, files in os.walk(SRC):
        if "i18n" in base.replace("\\", "/").split("/"):
            continue  # 词条表自身不参与收集
        for f in files:
            if not f.endswith((".vue", ".ts")):
                continue
            text = io.open(os.path.join(base, f), encoding="utf-8").read()
            for m in KEY_USE.finditer(text):
                keys.add(m.group(1).replace("\\'", "'").replace("\\\\", "\\"))
    return sorted(keys)


def read_catalog(lang):
    """从 TS 词条文件里解析出 {key: value}（格式受我们控制，用正则足够）"""
    path = os.path.join(MSG_DIR, f"{lang}.ts")
    if not os.path.exists(path):
        return {}
    raw = io.open(path, encoding="utf-8").read()
    body = raw.split("export default", 1)[-1]
    out = {}
    # 形如  '原文': '译文',   或  "原文": "译文",
    for m in re.finditer(r"^\s*(['\"])((?:\\.|(?!\1).)*)\1\s*:\s*(['\"])((?:\\.|(?!\3).)*)\3\s*,?\s*$", body, re.M):
        key = m.group(2).replace("\\'", "'").replace('\\"', '"')
        val = m.group(4).replace("\\'", "'").replace('\\"', '"')
        out[key] = val
    return out


def emit_zh(keys):
    lines = [
        "/**",
        " * 简体中文词条表：值即 key 本身。",
        " *",
        " * 这张表**必须完整**（由 scripts/i18n_catalog.py --emit-zh 自动生成）：",
        " * vue-i18n 在找不到消息时会原样返回 key 且不做插值，界面会出现 {a} 这类字面量；",
        " * 收录后既能让插值生效，也为 en/ja/ko 提供回退文案。",
        " */",
        "export default {",
    ]
    for k in keys:
        escaped = k.replace("\\", "\\\\").replace("'", "\\'")
        lines.append(f"  '{escaped}': '{escaped}',")
    lines.append("} as Record<string, string>")
    lines.append("")
    io.open(os.path.join(MSG_DIR, "zh-CN.ts"), "w", encoding="utf-8").write("\n".join(lines))


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    keys = collect_keys()
    print(f"代码中使用的 key 共 {len(keys)} 条")

    if mode == "--emit-zh":
        emit_zh(keys)
        print(f"已生成 {os.path.relpath(os.path.join(MSG_DIR, 'zh-CN.ts'), ROOT)}（{len(keys)} 条）")
        return 0

    ok = True
    zh = read_catalog("zh-CN")
    missing_zh = [k for k in keys if k not in zh]
    print(f"zh-CN 覆盖：{len(keys) - len(missing_zh)}/{len(keys)}")
    if missing_zh:
        ok = False
        print(f"  ✗ zh-CN 缺少 {len(missing_zh)} 条（会导致插值失效），示例：{missing_zh[:3]}")
        print("    → 运行 python scripts/i18n_catalog.py --emit-zh 修复")

    for lang in LANGS[1:]:
        cat = read_catalog(lang)
        missing = [k for k in keys if k not in cat]
        print(f"{lang} 覆盖：{len(keys) - len(missing)}/{len(keys)}（缺 {len(missing)}）")
        if missing:
            ok = False

        # 占位符一致性：漏掉 {a}/{b} 会让界面少显示数据
        bad = []
        for k, v in cat.items():
            base = set(PLACEHOLDER.findall(k))
            got = set(PLACEHOLDER.findall(v))
            # 译文可能用 {a} 之外的命名，只要求"占位符数量与名称集合"一致
            if base != got:
                bad.append((k, sorted(base), sorted(got)))
        if bad:
            ok = False
            print(f"  ✗ {lang} 有 {len(bad)} 条占位符不一致，示例：{bad[:3]}")

    print("\n结论：" + ("✅ 词条表完整且占位符一致" if ok else "❌ 存在缺口（上方已列出）"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
