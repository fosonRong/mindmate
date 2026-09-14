#!/usr/bin/env node
/**
 * 生成自动更新清单（latest.json）与校验文件（SHA256SUMS.txt），并把产物收集到 release/site（供静态托管）。
 *
 * 用法：node scripts/make-manifest.mjs
 *   ARTIFACTS_DIR      CI 下载下来的各平台产物目录（如 ./artifacts）；本地开发可省略，
 *                      脚本会直接读 src-tauri/target 下的构建产物
 *   RELEASE_BASE_URL   产物下载前缀，如 https://mindmate.pages.dev（缺失则用占位并告警）
 *   RELEASE_NOTES      更新说明（缺省读环境变量或用版本号）
 *   CI=1               在 CI 里运行时，缺少更新签名视为失败（否则用户装不上更新，必须当红灯）
 *
 * 产物：
 *   release/latest.json        Tauri updater 清单（按平台给 URL 与签名）
 *   release/SHA256SUMS.txt     所有安装包校验值
 *   release/site/              待部署到 Cloudflare Pages 的目录（清单 + 安装包 + 下载页）
 *
 * 平台键（必须与 Tauri updater 的识别名一致）：
 *   windows-x86_64 / darwin-aarch64（Apple 芯片）/ darwin-x86_64（Intel）
 *
 * 注意：updater 只升不降 —— 回滚请发布一个版本号更高的补丁版本，而不是把本清单改回旧版本。
 */
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import path from 'node:path'

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '..')
const outDir = path.join(root, 'release')
const siteDir = path.join(outDir, 'site')
const baseUrl = (process.env.RELEASE_BASE_URL || '').replace(/\/+$/, '')
const inCI = !!process.env.CI

if (!baseUrl) {
  console.warn('⚠️  未设置 RELEASE_BASE_URL，清单里的下载地址会是占位值，客户端将无法更新')
}

/** 版本号与产品名取自 tauri.conf.json，保持单一来源 */
const conf = JSON.parse(readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'))
const version = conf.version
const notes = process.env.RELEASE_NOTES || `智伴 Mindmate ${version}`

/** 递归列出目录下所有文件（跳过 .git 等） */
function walk(dir, out = []) {
  if (!existsSync(dir)) return out
  for (const name of readdirSync(dir)) {
    const p = path.join(dir, name)
    let st
    try {
      st = statSync(p)
    } catch {
      continue
    }
    if (st.isDirectory()) {
      if (name === '.git' || name === 'node_modules') continue
      walk(p, out)
    } else {
      out.push(p)
    }
  }
  return out
}

/** 产物搜索根：CI 用下载目录，本地用构建目录 + 绿色版目录 */
function candidateFiles() {
  const dirs = []
  if (process.env.ARTIFACTS_DIR) dirs.push(path.resolve(root, process.env.ARTIFACTS_DIR))
  dirs.push(path.join(root, 'src-tauri', 'target'))
  return dirs.flatMap((d) => walk(d))
}

/** 读取 Tauri 生成的更新签名（createUpdaterArtifacts=true 时产出 <文件>.sig） */
function readSignature(file) {
  const sigPath = `${file}.sig`
  if (!existsSync(sigPath)) return null
  return readFileSync(sigPath, 'utf8').trim()
}

const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex')

/** 从路径推断 macOS 架构。
 *
 * ⚠️ 不要只认 cargo 三元组：CI 用 actions/download-artifact 下载后，目录名是
 * `artifacts/darwin-aarch64/...`，而不是 `.../aarch64-apple-darwin/...`。
 * 早期实现只匹配三元组，两个架构于是都落到 fallback、被当成同一个架构 ——
 * 结果是清单里只剩 x86_64，Apple 芯片用户永远收不到更新，而且**不报任何错**
 * （v1.0.11 发布时实际发生）。这里按「先 arm，再 x86」判断，覆盖两种目录布局。
 */
function macArch(file) {
  const p = file.replace(/\\/g, '/').toLowerCase()
  if (/aarch64|arm64/.test(p)) return 'aarch64'
  if (/x86_64|x64|amd64/.test(p)) return 'x86_64'
  return process.arch === 'arm64' ? 'aarch64' : 'x86_64'
}

function resetSite() {
  // 只清站点目录与本脚本产出的两个文件，不要动 release/ 下的其它内容
  rmSync(siteDir, { recursive: true, force: true })
  mkdirSync(siteDir, { recursive: true })
  for (const f of ['latest.json', 'SHA256SUMS.txt']) rmSync(path.join(outDir, f), { force: true })
}

function main() {
  const files = candidateFiles()
  const pick = (re) => files.filter((f) => re.test(f.replace(/\\/g, '/')))

  // 选文件时的两个坑：
  //   1) 本地 target 目录会残留历史版本的安装包（1.0.0 曾被抓进 1.0.10 的清单）；
  //   2) macOS 的 Mindmate.app.tar.gz 文件名不含版本号，无法据此区分。
  // 因此：文件名含当前版本者优先，同级再取修改时间最新的。
  const newest = (list) => list.slice().sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0]
  const best = (re) => {
    const m = pick(re)
    if (!m.length) return undefined
    const withVer = m.filter((f) => path.basename(f).includes(version))
    return newest(withVer.length ? withVer : m)
  }

  const nsisExe = best(/Mindmate[^/]*setup\.exe$/i)

  // macOS：按架构各取一个最新的（两个架构的 .app.tar.gz / .dmg 同名，只靠路径里的三元组区分）
  const byArch = (re) => {
    const out = {}
    for (const f of pick(re)) {
      const a = macArch(f)
      if (!out[a] || statSync(f).mtimeMs > statSync(out[a]).mtimeMs) out[a] = f
    }
    return out
  }
  const appTarballs = byArch(/\.app\.tar\.gz$/)
  const dmgs = byArch(/\.dmg$/)

  if (!nsisExe && Object.keys(appTarballs).length === 0) {
    console.error('✗ 未找到任何平台构建产物（Windows: *setup.exe；macOS: *.app.tar.gz）')
    console.error('  请先运行 npm run desktop:build（CI 请确认 ARTIFACTS_DIR 指向下载目录）')
    process.exit(1)
  }

  // 关键校验：两个 macOS 架构必须各有一个产物。少一个就意味着对应机型的用户
  // 永远收不到更新（清单里没有 darwin-<arch> 时 updater 只是"没有可用更新"，不会报错），
  // 所以这里必须当红灯而不是静默跳过。CI 上传产物时就已经用 if-no-files-found: error
  // 保证文件存在，这里再兜一道，防止"文件在但架构被识别成同一个"。
  const missingArch = ['aarch64', 'x86_64'].filter((a) => !appTarballs[a])
  if (Object.keys(appTarballs).length > 0 && missingArch.length) {
    console.error(`✗ macOS 缺少架构：${missingArch.join(', ')}（清单里没有该键，对应机型收不到更新）`)
    console.error(`  已识别到的产物：${Object.entries(appTarballs).map(([a, f]) => `${a}=${path.basename(f)}`).join(', ')}`)
    process.exit(1)
  }
  for (const [arch, f] of Object.entries(appTarballs)) {
    if (!f.endsWith('.sig') && !existsSync(`${f}.sig`)) {
      console.error(`✗ macOS ${arch} 缺少更新签名：${path.basename(f)}.sig（客户端会拒绝安装）`)
      process.exit(1)
    }
  }

  if (nsisExe && !path.basename(nsisExe).includes(version)) {
    console.warn(`⚠️  安装包文件名里没有当前版本 ${version}：${path.basename(nsisExe)}（确认不是旧产物？）`)
  }

  resetSite()

  // ── 收集产物并统一命名（两个 macOS 架构的 app.tar.gz 同名，必须区分开）──
  const sums = []
  const collect = (file, name) => {
    cpSync(file, path.join(siteDir, name))
    sums.push(`${sha256(file)}  ${name}`)
    return { name, size: statSync(file).size, sig: readSignature(file) }
  }

  const platforms = {}
  const missingSig = []
  const downloads = [] // 下载页展示用（分组）

  const url = (name) => `${baseUrl || 'https://REPLACE_ME.pages.dev'}/${name}`

  // Windows：NSIS 安装包（updater 用）
  if (nsisExe) {
    const name = path.basename(nsisExe)
    const a = collect(nsisExe, name)
    if (!a.sig) missingSig.push('windows-x86_64')
    platforms['windows-x86_64'] = { signature: a.sig || '', url: url(name) }
    downloads.push({ group: 'Windows', label: '安装版（推荐）', name, size: a.size })
  }

  // macOS：.app.tar.gz（updater 用）+ .dmg（手动安装用）
  const archLabel = { aarch64: 'Apple 芯片（M 系列）', x86_64: 'Intel 芯片' }
  for (const [arch, t] of Object.entries(appTarballs)) {
    const key = `darwin-${arch}`
    const name = `Mindmate_${version}_${arch}.app.tar.gz`
    const a = collect(t, name)
    if (!a.sig) missingSig.push(key)
    platforms[key] = { signature: a.sig || '', url: url(name) }
  }
  for (const [arch, d] of Object.entries(dmgs)) {
    const name = `Mindmate_${version}_${arch}.dmg`
    const a = collect(d, name)
    downloads.push({ group: 'macOS', label: archLabel[arch] || arch, name, size: a.size })
  }


  // ── 更新清单 ──
  const manifest = { version, notes, pub_date: new Date().toISOString(), platforms }
  const manifestJson = JSON.stringify(manifest, null, 2)
  writeFileSync(path.join(outDir, 'latest.json'), manifestJson)
  writeFileSync(path.join(siteDir, 'latest.json'), manifestJson)
  // 规范位置：/release/latest.json
  // 原因：Pages 生产别名对已存在的路径可能长期持有旧对象（旧清单导致客户端永远收不到更新）。
  // 换到全新路径 + no-store 头，二者结合可确保清单始终最新。
  mkdirSync(path.join(siteDir, 'release'), { recursive: true })
  writeFileSync(path.join(siteDir, 'release', 'latest.json'), manifestJson)

  // ── 校验文件 ──
  const sumsText = sums.join('\n') + '\n'
  writeFileSync(path.join(outDir, 'SHA256SUMS.txt'), sumsText)
  writeFileSync(path.join(siteDir, 'SHA256SUMS.txt'), sumsText)

  // ── 极简下载页（无构建、无依赖）──
  const groups = [...new Set(downloads.map((d) => d.group))]
  const sections = groups
    .map((g) => {
      const rows = downloads
        .filter((d) => d.group === g)
        .map(
          (d) =>
            `<li><a href="./${d.name}">${d.label}</a> <span class="muted">${d.name} · ${(d.size / 1048576).toFixed(2)} MB</span></li>`
        )
        .join('\n      ')
      return `<h3>${g}</h3>\n  <ul>\n      ${rows}\n  </ul>`
    })
    .join('\n  ')

  const macNote = downloads.some((d) => d.group === 'macOS')
    ? `<p class="muted">macOS 说明：当前安装包<b>未经 Apple 签名/公证</b>（暂无开发者账号）。首次打开若提示"已损坏"或"无法验证开发者"，
       请右键点图标选「打开」，或在终端执行一次：<code>xattr -dr com.apple.quarantine /Applications/Mindmate.app</code>。
       Apple 芯片选 aarch64，Intel 机型选 x86_64。</p>`
    : ''

  const html = `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>智伴 Mindmate ${version}</title>
<style>
  body { font: 15px/1.7 -apple-system, "Segoe UI", system-ui, sans-serif; max-width: 760px; margin: 40px auto; padding: 0 20px; color: #1f2430; }
  h1 { font-size: 24px; } h3 { margin-bottom: 4px; } .muted { color: #6b7280; font-size: 13px; }
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
  ${sections}
  <h2>校验（建议安装前核对）</h2>
  <pre>${sumsText}</pre>
  ${macNote}
  <p class="muted">已安装的用户会在应用内自动收到更新提示，无需手动下载。</p>
  <footer class="muted">
    <p>隐私：你的记录与待办只保存在本机；本页仅提供程序下载与版本信息。</p>
  </footer>
</body>
</html>`
  writeFileSync(path.join(siteDir, 'index.html'), html)

  // ── Cloudflare Pages 缓存策略（关键）──
  // 坑：生产别名（*.pages.dev）会缓存静态文件，`latest.json` 若被缓存，客户端会一直看到旧版本 ——
  // 表现为"发了新版本但用户永远收不到更新"。因此清单与校验文件必须 no-store，
  // 而带版本号的安装包可以长期缓存（文件名变了，天然不受影响）。
  const headers = `# 由 scripts/make-manifest.mjs 生成
/latest.json
  Cache-Control: no-store

/release/latest.json
  Cache-Control: no-store

/SHA256SUMS.txt
  Cache-Control: no-store

/index.html
  Cache-Control: no-cache

/*.exe
  Cache-Control: public, max-age=31536000, immutable

/*.dmg
  Cache-Control: public, max-age=31536000, immutable

/*.tar.gz
  Cache-Control: public, max-age=31536000, immutable
`
  writeFileSync(path.join(siteDir, '_headers'), headers)

  // ── 汇总 ──
  console.log(`✓ 版本 ${version}`)
  console.log(`✓ 平台：${Object.keys(platforms).sort().join(', ') || '（无）'}`)
  console.log(`✓ release/latest.json（${Object.keys(platforms).length} 个平台）`)
  console.log(`✓ release/SHA256SUMS.txt（${sums.length} 个文件）`)
  console.log(`✓ release/site/ 待部署（${readdirSync(siteDir).length} 个文件）`)
  if (missingSig.length) {
    console.error(`✗ 以下平台缺少更新签名（客户端会拒绝安装）：${missingSig.join(', ')}`)
    console.error('  请在构建时提供 TAURI_SIGNING_PRIVATE_KEY，并确认 bundle.createUpdaterArtifacts=true')
    if (inCI) process.exit(1)
  }
}

main()
