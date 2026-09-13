#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
i18n 迁移工具（模板文案抽取 / 改写 / 校验）

策略：**中文原文即消息 key**。
  - 模板文本节点      >日常记录<          → {{ $t('日常记录') }}
  - 带插值的文本节点  >共 {{ n }} 条<     → {{ $t('共 {a} 条', { a: n }) }}
  - 展示类属性        title="上个月"      → :title="$t('上个月')"
仅处理 <template> 段，且属性只认白名单（title/placeholder/aria-label/alt），
因此 v-if / :class 等**逻辑比较用的字面量不会被改写**（那里必须保留中文原值，用于匹配数据库里的规范值）。

用法：
  python scripts/i18n_tool.py --apply     # 就地改写（会打印每个文件的改动数）
  python scripts/i18n_tool.py --keys      # 打印当前代码里用到的全部 key
  python scripts/i18n_tool.py --check     # 校验残留（模板里是否还有未抽取的中文）
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
CJK = re.compile(r"[\u4e00-\u9fff]")
# 文本节点：>文本<（不含子标签）
TEXT_NODE = re.compile(r">([^<>]*?)<")
# 模板属性（仅展示类）
ATTR = re.compile(r'(?<![:\w-])(title|placeholder|aria-label|alt)="([^"]*)"')
# 动态展示属性 :title="'文本'"
DYN_ATTR = re.compile(r':(title|placeholder|aria-label|alt)="\'([^\']*)\'"')
# 已抽取的 key（前置边界 (?<![.\w]) 很重要：否则 split('/x')/api.get('/x')/mount('#app') 会被误判为文案）
KEY_USE = re.compile(r"(?<![.\w])\$?t\(\s*'((?:[^'\\]|\\.)*)'")
ATTR_SKIP = {"class", "style", "v-if", "v-else-if", "v-for", "v-model", "v-show", "v-html", "ref", "key"}


def esc(s: str) -> str:
    return s.replace("\\", "\\\\").replace("'", "\\'")


def unprotect_body(text: str) -> str:
    """文本节点内容里被保护的 > 还原（插值表达式需要原样 emit）"""
    return text.replace(GT, ">")


def split_exprs(text: str):
    """把 '共 {{ n }} 条' 拆成 key 模板与表达式列表"""
    parts, exprs = [], []
    pos = 0
    for m in re.finditer(r"\{\{(.*?)\}\}", text, re.S):
        parts.append(text[pos:m.start()])
        exprs.append(m.group(1).strip())
        pos = m.end()
    parts.append(text[pos:])
    if not exprs:
        return text, []
    names = "abcdefghij"
    key = ""
    for i, p in enumerate(parts):
        key += p
        if i < len(exprs):
            key += "{%s}" % names[i]
    return key, exprs


def template_spans(src: str):
    """返回 <template> ... </template> 的区间（支持嵌套，用栈匹配）"""
    spans, stack = [], []
    for m in re.finditer(r"<template(\s[^>]*)?>|</template>", src):
        if m.group(0).startswith("</"):
            if stack:
                start = stack.pop()
                if not stack:  # 最外层
                    spans.append((start, m.end()))
        else:
            stack.append(m.start())
    return spans


# 受保护的 `>`（插值表达式/属性值内部），避免被当成标签边界
GT = "\x01GT\x01"


def protect_gt(block: str) -> str:
    """把 {{ … }} 与 "…" 内部的 > 暂时替换掉（=> / >= 会命中）"""
    out = []
    i = 0
    n = len(block)
    while i < n:
        if block.startswith("{{", i):
            j = block.find("}}", i)
            if j == -1:
                out.append(block[i])
                i += 1
                continue
            out.append(block[i:j + 2].replace(">", GT))
            i = j + 2
        elif block[i] == '"':
            j = block.find('"', i + 1)
            if j == -1:
                out.append(block[i])
                i += 1
                continue
            out.append(block[i:j + 1].replace(">", GT))
            i = j + 1
        else:
            out.append(block[i])
            i += 1
    return "".join(out)


def unprotect(s: str) -> str:
    return s.replace(GT, ">")


# 插值内的中文字面量：仅当"不是比较/匹配用的规范值"时才翻译
CMP_BEFORE = re.compile(r"(===|!==|==|!=|includes\(|indexOf\(|startsWith\(|endsWith\(|match\(|test\(|case\s)\s*$")
# 比较运算符后紧跟的中文字面量（=== '日程'）—— 数据库规范值，按设计不翻译
COMPARE_LITERAL = re.compile(r"(===|!==|==|!=)\s*'[^']*[\u4e00-\u9fff][^']*'")
STR_LIT = re.compile(r"'((?:[^'\\]|\\.)*)'")


def rewrite_exprs(block: str, counter: list) -> str:
    """把 {{ … }} 里的中文字符串字面量包成 $t('…')；跳过比较/匹配上下文。

    例：{{ a ? '保存中…' : '保存' }}          → {{ a ? $t('保存中…') : $t('保存') }}
        {{ todo.category === '日程' && x }}    → 保持不变（'日程' 是数据库规范值，用于比较）
    """
    out, i = [], 0
    while True:
        start = block.find("{{", i)
        if start == -1:
            out.append(block[i:])
            break
        end = block.find("}}", start)
        if end == -1:
            out.append(block[i:])
            break
        out.append(block[i:start])
        expr = block[start:end]
        changed_expr, n = _translate_literals(expr)
        counter[0] += n
        out.append(changed_expr)
        i = end
    return "".join(out)


def _translate_literals(expr: str) -> tuple:
    changed = 0

    def repl(m):
        nonlocal changed
        lit = m.group(1)
        if not CJK.search(lit):
            return m.group(0)
        before = expr[: m.start()]
        if CMP_BEFORE.search(before):
            return m.group(0)  # 比较用的规范值，禁止翻译（否则筛选/判断会失效）
        if before.rstrip().endswith("$t(") or before.rstrip().endswith("t("):
            return m.group(0)  # 已包裹，避免 $t($t(…)) 双重翻译（踩过一次）
        changed += 1
        return "$t('%s')" % esc(lit.replace("\'", "'"))

    return STR_LIT.sub(repl, expr), changed


def rewrite_template(block: str, counter: list):
    changed = 0
    block = protect_gt(block)

    def repl_node(m):
        nonlocal changed
        raw = m.group(1)
        if not CJK.search(raw) or not raw.strip():
            return m.group(0)
        lead = raw[: len(raw) - len(raw.lstrip())]
        trail = raw[len(raw.rstrip()):]
        body = raw.strip()
        if body.startswith("{{") and body.endswith("}}") and body.count("{{") == 1:
            return m.group(0)  # 纯表达式，不动
        # 关键：折叠换行与连续空白 —— 文本节点可能跨行，直接塞进 $t('…') 会产生
        # 字符串里的裸换行，导致「Unterminated string literal」语法错误（踩过一次）
        body = " ".join(body.split())
        if body.count("{{") != body.count("}}"):
            return m.group(0)  # 插值不配对，保守跳过
        key, exprs = split_exprs(unprotect_body(body))
        names = "abcdefghij"
        if exprs:
            params = ", ".join(f"{names[i]}: {e}" for i, e in enumerate(exprs))
            out = "{{ $t('%s', { %s }) }}" % (esc(key), params)
        else:
            out = "{{ $t('%s') }}" % esc(key)
        changed += 1
        return ">" + lead + out + trail + "<"

    block = TEXT_NODE.sub(repl_node, block)

    def repl_attr(m):
        nonlocal changed
        name, val = m.group(1), m.group(2)
        if not CJK.search(val):
            return m.group(0)
        changed += 1
        return ':%s="$t(\'%s\')"' % (name, esc(val))

    block = ATTR.sub(repl_attr, block)

    def repl_dyn(m):
        nonlocal changed
        changed += 1
        return ':%s="$t(\'%s\')"' % (m.group(1), esc(m.group(2)))

    block = DYN_ATTR.sub(repl_dyn, block)
    block = rewrite_exprs(block, counter)
    return unprotect(block)


def rewrite_vue(path: str, apply: bool):
    src = io.open(path, encoding="utf-8").read()
    spans = template_spans(src)
    if not spans:
        return 0
    out, prev = "", 0
    counter = [0]
    for a, b in spans:
        out += src[prev:a] + rewrite_template(src[a:b], counter)
        prev = b
    out += src[prev:]
    if apply and counter[0]:
        io.open(path, "w", encoding="utf-8", newline="").write(out)
    return counter[0]


def vue_files():
    for base, _d, files in os.walk(SRC):
        for f in sorted(files):
            if f.endswith(".vue"):
                yield os.path.join(base, f)


def all_src_files():
    for base, _d, files in os.walk(SRC):
        for f in sorted(files):
            if f.endswith((".vue", ".ts")):
                yield os.path.join(base, f)


def collect_keys():
    keys = set()
    for p in all_src_files():
        s = io.open(p, encoding="utf-8").read()
        for m in KEY_USE.finditer(s):
            keys.add(m.group(1).replace("\\'", "'").replace("\\\\", "\\"))
    return keys


def check_leftover():
    """模板里是否还有**未抽取**的中文（已用 $t() 包裹的不算）

    注意：抽取后文本节点内容形如 {{ $t('刷新') }}，本身仍含中文，
    必须排除，否则校验恒为"有残留"，失去意义。
    """
    bad = []
    for p in vue_files():
        s = io.open(p, encoding="utf-8").read()
        for a, b in template_spans(s):
            block = s[a:b]
            for m in TEXT_NODE.finditer(block):
                txt = m.group(1)
                if not txt.strip() or not CJK.search(txt):
                    continue
                if "$t(" in txt or " t(" in txt:
                    continue  # 已抽取
                # 过滤"比较用的规范值"（如 === '日程' / === '已完成'）：这些是数据库取值，
                # 按设计**不翻译**，因此不算残留，否则校验会长期报假阳性。
                if COMPARE_LITERAL.search(txt):
                    continue
                bad.append((os.path.relpath(p, ROOT).replace("\\", "/"), txt.strip()[:48]))
            for m in ATTR.finditer(block):
                if CJK.search(m.group(2)):
                    bad.append((os.path.relpath(p, ROOT).replace("\\", "/"), "%s=%s" % (m.group(1), m.group(2)[:30])))
    return bad


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    if mode == "--apply":
        total = 0
        for p in vue_files():
            n = rewrite_vue(p, True)
            if n:
                print(f"  {n:4d}  {os.path.relpath(p, ROOT).replace(chr(92), '/')}")
                total += n
        print(f"共改写 {total} 处模板文案")
    elif mode == "--keys":
        keys = sorted(collect_keys())
        print(json.dumps(keys, ensure_ascii=False, indent=0))
    else:
        bad = check_leftover()
        if bad:
            print(f"模板中仍有 {len(bad)} 处未抽取的中文：")
            for f, t in bad[:40]:
                print(f"  {f}  →  {t}")
            sys.exit(1)
        print("模板文案已全部抽取 ✅")


if __name__ == "__main__":
    main()
