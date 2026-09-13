#!/usr/bin/env node
/**
 * 跨平台 cargo 包装器：把用户级 ~/.cargo/bin 补进 PATH 后调用 cargo。
 * 用于 npm scripts（npm 子进程不会继承手工添加的 PATH）。
 *
 * 用法：node scripts/run-cargo.mjs test --manifest-path core/Cargo.toml
 */
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { homedir } from 'node:os'
import { delimiter, join } from 'node:path'

const cargoBin = join(homedir(), '.cargo', 'bin')
const cargoExe = process.platform === 'win32' ? 'cargo.exe' : 'cargo'
const cargoPath = join(cargoBin, cargoExe)

if (existsSync(cargoBin)) {
  process.env.PATH = cargoBin + delimiter + (process.env.PATH || '')
}

const cargo = existsSync(cargoPath) ? cargoPath : 'cargo'
const child = spawn(cargo, process.argv.slice(2), { stdio: 'inherit', env: process.env })

child.on('error', (e) => {
  console.error(`\n无法启动 cargo（${cargo}）：${e.message}`)
  console.error('请安装 Rust 工具链：https://rustup.rs')
  process.exit(1)
})
child.on('exit', (code) => process.exit(code ?? 1))
