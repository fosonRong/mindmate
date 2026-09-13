#!/usr/bin/env node
/**
 * Tauri CLI 包装脚本（供根目录 npm scripts 使用）。
 *
 * 背景：Tauri 项目位于仓库根目录的 `src-tauri/`，而 `@tauri-apps/cli` 只装在
 * `apps/web` 工作区。直接在根目录写 `apps/web/node_modules/.bin/tauri` 在 Windows 上
 * 依赖 .cmd 解析、且不同 npm 版本对 `npm run --workspace <ws> exec` 支持不一致
 * （曾出现 `Missing script: "exec"`）。这里统一解析 CLI 入口，并把工作目录固定为
 * 仓库根目录，保证 `tauri dev/build` 都能找到 src-tauri/tauri.conf.json。
 *
 * 用法：node scripts/tauri-cli.mjs build | dev | info
 */
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const cliEntry = path.join(root, 'apps', 'web', 'node_modules', '@tauri-apps', 'cli', 'tauri.js')

if (!existsSync(cliEntry)) {
  console.error('未找到 Tauri CLI，请先执行：npm install')
  process.exit(1)
}

// cargo 常为当前用户级安装（不在 PATH），显式补上 ~/.cargo/bin
const cargoBin = path.join(process.env.USERPROFILE || process.env.HOME || '', '.cargo', 'bin')
const env = { ...process.env, PATH: cargoBin + path.delimiter + (process.env.PATH || '') }

const child = spawn(process.execPath, [cliEntry, ...process.argv.slice(2)], {
  cwd: root,
  stdio: 'inherit',
  env
})
child.on('exit', (code) => process.exit(code ?? 0))
