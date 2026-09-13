#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
一键配置 GitHub 仓库的发版参数（Actions Secrets 与 Variables）

为什么需要它：CI 发版需要 4 个 Secret + 2 个 Variable（更新签名私钥、Cloudflare 凭据等），
手工在网页上逐个粘贴又慢又容易漏。本脚本用仓库公钥（libsodium sealed box）加密后写入。

用法：
    set GITHUB_TOKEN=ghp_xxx            # 需要 repo + workflow 权限
    set GITHUB_REPO=fosonRong/mindmate
    python scripts/set-github-secrets.py            # 写入脚本内已知的项目参数
    python scripts/set-github-secrets.py --check    # 只列出当前已配置的名称

可选环境变量：
    CF_API_TOKEN     若提供，则一并写入 CLOUDFLARE_API_TOKEN
依赖：pip install pynacl requests（requests 可省，脚本用标准库 urllib）
"""
import base64
import io
import json
import os
import sys
import urllib.error
import urllib.request

for s in (sys.stdout, sys.stderr):
    try:
        s.reconfigure(encoding="utf-8")
    except Exception:
        pass

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
API = "https://api.github.com"

# 本项目的固定参数（改动时同步这里与 docs/发布与更新文档.md）
PAGES_PROJECT = "mindmate"
RELEASE_BASE_URL = "https://production.mindmate-10v.pages.dev"
CLOUDFLARE_ACCOUNT_ID = "dd67b277119c2a0643c7b3f039d15976"
SIGNING_KEY_PATH = os.path.join(ROOT, ".tauri", "mindmate.key")


def token() -> str:
    t = os.environ.get("GITHUB_TOKEN", "").strip()
    if not t:
        print("✗ 缺少 GITHUB_TOKEN 环境变量")
        sys.exit(1)
    return t


def repo() -> str:
    r = os.environ.get("GITHUB_REPO", "").strip()
    if not r:
        print("✗ 缺少 GITHUB_REPO 环境变量（形如 owner/name）")
        sys.exit(1)
    return r


def call(method: str, path: str, body=None):
    url = f"{API}{path}"
    data = json.dumps(body).encode("utf-8") if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", f"Bearer {token()}")
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("X-GitHub-Api-Version", "2022-11-28")
    if data:
        req.add_header("Content-Type", "application/json")
    # 走本机代理（国内网络下 GitHub 直连常被重置）
    proxy = os.environ.get("HTTPS_PROXY") or os.environ.get("https_proxy") or "http://127.0.0.1:7897"
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({"http": proxy, "https": proxy}))
    try:
        with opener.open(req, timeout=30) as r:
            raw = r.read().decode("utf-8")
            return r.status, (json.loads(raw) if raw.strip() else {})
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", "replace")
        try:
            return e.code, json.loads(raw)
        except Exception:
            return e.code, {"message": raw[:200]}


def encrypt(public_key_b64: str, value: str) -> str:
    """GitHub 要求用仓库公钥做 libsodium sealed box 加密"""
    from nacl import encoding, public

    pk = public.PublicKey(public_key_b64.encode("utf-8"), encoding.Base64Encoder())
    sealed = public.SealedBox(pk)
    return base64.b64encode(sealed.encrypt(value.encode("utf-8"))).decode("utf-8")


def set_secret(name: str, value: str, key_id: str, key_b64: str) -> bool:
    code, _ = call("PUT", f"/repos/{repo()}/actions/secrets/{name}", {
        "encrypted_value": encrypt(key_b64, value),
        "key_id": key_id,
    })
    print(f"  {'✅' if code in (201, 204) else '❌'} Secret  {name}" + ("" if code in (201, 204) else f"  (HTTP {code})"))
    return code in (201, 204)


def set_variable(name: str, value: str) -> bool:
    repo_full = repo()
    code, _ = call("POST", f"/repos/{repo_full}/actions/variables", {"name": name, "value": value})
    if code == 409:  # 已存在 → 更新
        code, _ = call("PATCH", f"/repos/{repo_full}/actions/variables/{name}", {"name": name, "value": value})
    ok = code in (201, 204)
    print(f"  {'✅' if ok else '❌'} Variable {name}" + ("" if ok else f"  (HTTP {code})"))
    return ok


def main() -> int:
    if "--check" in sys.argv:
        code, data = call("GET", f"/repos/{repo()}/actions/secrets")
        if code != 200:
            print(f"✗ 读取失败 HTTP {code}: {data}")
            return 1
        names = [s["name"] for s in data.get("secrets", [])]
        code2, data2 = call("GET", f"/repos/{repo()}/actions/variables")
        vnames = [v["name"] for v in (data2.get("variables") or [])]
        need = ["TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
                "CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"]
        print("Secrets：")
        for n in need:
            print(f"  {'✅' if n in names else '⬜'} {n}")
        print("Variables：")
        for n in ["PAGES_PROJECT", "RELEASE_BASE_URL"]:
            print(f"  {'✅' if n in vnames else '⬜'} {n}")
        return 0

    print(f"目标仓库：{repo()}")
    code, info = call("GET", f"/repos/{repo()}/actions/secrets/public-key")
    if code != 200:
        print(f"✗ 取不到仓库公钥（HTTP {code}）：{info} —— 令牌缺少 repo/secrets 权限？")
        return 1
    key_id, key_b64 = info["key_id"], info["key"]

    ok = True
    if os.path.exists(SIGNING_KEY_PATH):
        ok &= set_secret("TAURI_SIGNING_PRIVATE_KEY", io.open(SIGNING_KEY_PATH, encoding="utf-8").read(), key_id, key_b64)
    else:
        print(f"  ❌ 未找到签名私钥 {SIGNING_KEY_PATH}（先运行 tauri signer generate）")
        ok = False
    ok &= set_secret("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "", key_id, key_b64)
    ok &= set_secret("CLOUDFLARE_ACCOUNT_ID", CLOUDFLARE_ACCOUNT_ID, key_id, key_b64)

    cf = os.environ.get("CF_API_TOKEN", "").strip()
    if cf:
        ok &= set_secret("CLOUDFLARE_API_TOKEN", cf, key_id, key_b64)
    else:
        print("  ⬜ Secret  CLOUDFLARE_API_TOKEN（未提供 CF_API_TOKEN，跳过）")

    ok &= set_variable("PAGES_PROJECT", PAGES_PROJECT)
    ok &= set_variable("RELEASE_BASE_URL", RELEASE_BASE_URL)

    print("\n完成。" if ok else "\n部分步骤失败，请检查上方输出。")
    print("提示：用 python scripts/set-github-secrets.py --check 查看配置状态")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
