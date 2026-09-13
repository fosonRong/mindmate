#!/usr/bin/env node
/**
 * 生成自动更新清单（latest.json）与校验文件（SHA256SUMS.txt），并把产物收集到 release/site（供静态托管）。
 *
 * 用法：node scripts/make-manifest.mjs
 *   RELEASE_BASE_URL  产物下载前缀，如 https://mindmate.pages.dev（默认读环境变量，缺失则用占位并告警）
 *   RELEASE_NOTES     更新说明（缺省读环境变量或用版本号）
 *
 * 产物：
 *   release/latest.json       Tauri updater 清单（含各平台 URL 与签名）
 *   release/SHA256SUMS.txt    安装包/绿色版校验值
 *   release/site/             待部署到 Cloudflare Pages 的目录（清单 + 安装包 + 下载页）
 *
 * 注意：updater 只升不降 —— 回滚请发布一个版本号更高的补丁版本，而不是把本清单改回旧版本。
 */
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import path from 'node:path'

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '..')
const bundleDir = path.join(root, 'src-tauri', 'target', 'release', 'bundle', 'nsis')
const targetDir = path.join(root, 'src-tauri', 'target', 'release')
const outDir = path.join(root, 'release')
const siteDir = path.join(outDir, 'site')

const baseUrl = (process.env.RELEASE_BASE_URL || '').replace(/\/+$/, '')
if (!baseUrl) {
  console.warn('⚠️  未设置 RELEASE_BASE_URL，清单里的下载地址会是占位值，客户端将无法更新')
}

/** 版本号与产品名取自 tauri.conf.json，保持单一来源 */
const conf = JSON.parse(readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'))
const version = conf.version
const notes = process.env.RELEASE_NOTES || `智伴 Mindmate ${version}`

/** 读取 Tauri 生成的更新签名（<安装包>.sig，构建时开启 createUpdaterArtifacts 才会产出） */
function readSignature(file) {
  const sigPath = `${file}.sig`
  if (!existsSync(sigPath)) return null
  return readFileSync(sigPath, 'utf8').trim()
}

function sha256(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex')
}

function reset(dir) {
  rmSync(dir, { recursive: true, force: true })
  mkdirSync(dir, { recursive: true })
}

function main() {
  if (!existsSync(bundleDir)) {
    console.error(`✗ 未找到构建产物目录：${bundleDir}（请先运行 npm run desktop:build）`)
    process.exit(1)
  }
  reset(outDir)
  mkdirSync(siteDir, { recursive: true })

  const setupExe = readdirSync(bundleDir).find((f) => f.endsWith('.exe'))
  if (!setupExe) {
    console.error('✗ 未找到 NSIS 安装包（*.exe）')
    process.exit(1)
  }
  const setupPath = path.join(bundleDir, setupExe)
  const signature = readSignature(setupPath)
  if (!signature) {
    console.warn(
      '⚠️  未找到更新签名（*.sig）。请在 tauri.conf.json 中设置 bundle.createUpdaterArtifacts=true，' +
        '并在构建时提供 TAURI_SIGNING_PRIVATE_KEY，否则客户端会拒绝安装更新。'
    )
  }

  // ── 更新清单 ──
  const manifest = {
    version,
    notes,
    pub_date: new Date().toISOString(),
    platforms: {
      'windows-x86_64': {
        signature: signature || '',
        url: `${baseUrl || 'https://REPLACE_ME.pages.dev'}/${setupExe}`
      }
    }
  }
  writeFileSync(path.join(outDir, 'latest.json'), JSON.stringify(manifest, null, 2))
  writeFileSync(path.join(siteDir, 'latest.json'), JSON.stringify(manifest, null, 2))

  // ── 校验文件 + 收集站点产物 ──
  const sums = []
  const copyInto = (file) => {
    const name = path.basename(file)
    cpSync(file, path.join(siteDir, name))
    sums.push(`${sha256(file)}  ${name}`)
    return { name, size: statSync(file).size }
  }
  const files = [copyInto(setupPath)]

  const portableDir = path.join(outDir, 'portable')
  if (existsSync(portableDir)) {
    for (const f of readdirSync(portableDir)) files.push(copyInto(path.join(portableDir, f)))
  }
  writeFileSync(path.join(outDir, 'SHA256SUMS.txt'), sums.join('\n') + '\n')
  writeFileSync(path.join(siteDir, 'SHA256SUMS.txt'), sums.join('\n') + '\n')

  // ── 极简下载页（无构建、无依赖） ──
  const rows = files
    .map((f) => `<li><a href="./${f.name}">${f.name}</a> <span class="muted">${(f.size / 1048576).toFixed(2)} MB</span></li>`)
    .join('\n      ')
  const html = `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>智伴 Mindmate ${version}</title>
<style>
  body { font: 15px/1.7 -apple-system, "Segoe UI", system-ui, sans-serif; max-width: 760px; margin: 40px auto; padding: 0 20px; color: #1f2430; }
  h1 { font-size: 24px; } .muted { color: #6b7280; font-size: 13px; }
  code, pre { background: #f4f5f8; border-radius: 6px; padding: 2px 6px; font-size: 13px; }
  pre { padding: 12px; overflow-x: auto; }
  footer { margin-top: 32px; border-top: 1px solid #e6e8ee; padding-top: 16px; }
</style>
</head>
<body>
  <h1>智伴 Mindmate <span class="muted">v${version}</span></h1>
  <p>AI 工作生活伴侣：记录、待办、提醒、进度统计与 AI 日报/周报/月报。数据全部保存在本机。</p>
  <p><strong>更新说明</strong>：${notes}</p>
  <h2>下载</h2>
  <ul>
      ${rows}
  </ul>
  <h2>校验（建议安装前核对）</h2>
  <pre>${sums.join('\n')}</pre>
  <p class="muted">已安装的用户会在应用内自动收到更新提示，无需手动下载。</p>
  <footer class="muted">
    <p>隐私：你的记录与待办只保存在本机；本页仅提供程序下载与版本信息。</p>
  </footer>
</body>
</html>`
  writeFileSync(path.join(siteDir, 'index.html'), html)

  console.log(`✓ 版本 ${version}`)
  console.log(`✓ release/latest.json（签名${signature ? '已' : '未'}包含）`)
  console.log(`✓ release/SHA256SUMS.txt（${sums.length} 个文件）`)
  console.log(`✓ release/site/ 待部署（${readdirSync(siteDir).length} 个文件）`)
}

main()
