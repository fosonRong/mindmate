#!/usr/bin/env node
/**
 * Tauri CLI 包装脚本（供根目录 npm scripts 使用）。
 *
 * 背景：Tauri 项目位于仓库根目录的 `src-tauri/`，而 `@tauri-apps/cli` 装在
 * `apps/web` 工作区。直接在根目录写 `apps/web/node_modules/.bin/tauri` 在 Windows 上
 * 依赖 .cmd 解析、且不同 npm 版本对 `npm run --workspace <ws> exec` 支持不一致
 * （曾出现 `Missing script: "exec"`）。这里统一解析 CLI 入口，并把工作目录固定为
 * 仓库根目录，保证 `tauri dev/build` 都能找到 src-tauri/tauri.conf.json。
 *
 * ⚠️ 不要写死单一路径：npm 的工作区提升位置会随「有无 lock 文件 / npm 版本 / 平台」变化。
 * CI 上曾因为仓库不再提交 package-lock.json、npm 把 @tauri-apps/cli 提升到根 node_modules，
 * 导致写死的 apps/web/node_modules/... 找不到 CLI，三个平台一起失败（报"未找到 Tauri CLI"）。
 * 因此这里按候选路径 + Node 模块解析依次尝试。
 *
 * 用法：node scripts/tauri-cli.mjs build | dev | info
 */
import { spawn } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { fileURLToPath, pathToFileURL } from 'node:url'
import path from 'node:path'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

/** 候选入口：工作区本地 → 根 node_modules（npm 提升）→ .bin → Node ESM/CJS 解析 */
function findCliEntry() {
  const candidates = [
    path.join(root, 'apps', 'web', 'node_modules', '@tauri-apps', 'cli', 'tauri.js'),
    path.join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js')
  ]
  for (const c of candidates) if (existsSync(c)) return c

  // 交给 Node 解析 package.json 的 main/bin（能覆盖 pnpm/yarn 等非提升布局）
  for (const from of [root, path.join(root, 'apps', 'web')]) {
    try {
      const require = createRequire(path.join(from, 'noop.js'))
      const pkgPath = require.resolve('@tauri-apps/cli/package.json')
      const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'))
      const bin = typeof pkg.bin === 'string' ? pkg.bin : (pkg.bin && pkg.bin.tauri) || pkg.main
      if (bin) {
        const entry = path.join(path.dirname(pkgPath), bin)
        if (existsSync(entry)) return entry
      }
    } catch {
      /* 继续尝试下一个位置 */
    }
  }

  // 最后尝试可执行文件（Windows 上是 tauri.cmd，Unix 上是符号链接）
  for (const bin of [
    path.join(root, 'apps', 'web', 'node_modules', '.bin', 'tauri'),
    path.join(root, 'node_modules', '.bin', 'tauri')
  ]) {
    for (const suffix of ['', '.cmd', '.exe']) {
      if (existsSync(bin + suffix)) return bin + suffix
    }
  }
  return null
}

const cliEntry = findCliEntry()
if (!cliEntry) {
  console.error('未找到 Tauri CLI，请先执行：npm install')
  console.error(`查找位置：${root}/node_modules 与 ${root}/apps/web/node_modules`)
  process.exit(1)
}

// cargo 常为当前用户级安装（不在 PATH），显式补上 ~/.cargo/bin
const cargoBin = path.join(process.env.USERPROFILE || process.env.HOME || '', '.cargo', 'bin')
const env = { ...process.env, PATH: cargoBin + path.delimiter + (process.env.PATH || '') }

// 构建一次失败只需要几秒就能看懂原因，但 Windows 上的报错是 "failed to remove file
// ... 拒绝访问 (os error 5)"——九成是用户还开着应用（exe 被占用）。这里提前给出人话提示。
if (process.argv.slice(2).some((a) => a === 'build' || a === 'dev') && process.platform === 'win32') {
  try {
    const { execFileSync } = await import('node:child_process')
    const out = execFileSync('tasklist', ['/FI', 'IMAGENAME eq mindmate.exe'], { encoding: 'utf8' })
    if (out.toLowerCase().includes('mindmate.exe')) {
      console.warn('')
      console.warn('⚠️  检测到 mindmate.exe 正在运行。Windows 下会因文件被占用而构建失败')
      console.warn('   （报错形如 "failed to remove file ... 拒绝访问 (os error 5)"）。')
      console.warn('   请先退出应用（托盘图标 → 退出），或在任务管理器结束 mindmate.exe 后重试。')
      console.warn('')
    }
  } catch {
    /* 查询失败不影响构建 */
  }
}

const isJs = cliEntry.endsWith('.js') || cliEntry.endsWith('.mjs') || cliEntry.endsWith('.cjs')
const child = spawn(isJs ? process.execPath : cliEntry, [...(isJs ? [cliEntry] : []), ...process.argv.slice(2)], {
  cwd: root,
  stdio: 'inherit',
  env
})
child.on('exit', (code) => process.exit(code ?? 0))
child.on('error', (e) => {
  console.error(`启动 Tauri CLI 失败：${e.message}`)
  process.exit(1)
})
