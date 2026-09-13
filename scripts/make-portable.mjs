#!/usr/bin/env node
/**
 * 生成绿色版（免安装 zip）：解压即用，覆盖"公司电脑没有安装权限"的用户。
 *
 * 用法：node scripts/make-portable.mjs
 * 产物：release/portable/Mindmate_<version>_x64_portable.zip
 */
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')), '..')
const exe = path.join(root, 'src-tauri', 'target', 'release', 'mindmate.exe')
const conf = JSON.parse(readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'))
const outDir = path.join(root, 'release', 'portable')

/** 用 zip 命令打包（Windows 10+ / Git Bash 通常自带；缺失时给出提示而不失败整个流水线） */
import { execFileSync } from 'node:child_process'

function main() {
  if (!existsSync(exe)) {
    console.error(`✗ 未找到可执行文件：${exe}（请先运行 npm run desktop:build）`)
    process.exit(1)
  }
  rmSync(outDir, { recursive: true, force: true })
  mkdirSync(outDir, { recursive: true })

  const readme = `智伴 Mindmate ${conf.version}（绿色版）

使用方式：解压后双击 mindmate.exe 即可，无需安装、不写注册表。
数据位置：%APPDATA%\\Mindmate（与安装版共用；如需多个副本并存请注意这一点）
说明：首次运行如遇 Windows SmartScreen 提示，请选择「更多信息 → 仍要运行」。
`
  const stage = path.join(outDir, 'stage')
  mkdirSync(stage, { recursive: true })
  writeFileSync(path.join(stage, '使用说明.txt'), readme, 'utf8')

  const zipName = `Mindmate_${conf.version}_x64_portable.zip`
  try {
    // 只打包 exe 与说明（-j 扁平化路径）
    execFileSync('tar', ['-a', '-c', '-f', path.join(outDir, zipName), '-C', path.join(root, 'src-tauri', 'target', 'release'), 'mindmate.exe', '-C', stage, '使用说明.txt'], { stdio: 'inherit' })
    console.log(`✓ release/portable/${zipName}`)
  } catch (e) {
    console.warn('⚠️  未能在本机生成 zip（可能缺少 tar/zip 工具）；CI 上使用 windows-latest 时可正常生成。', e.message)
  }
  rmSync(stage, { recursive: true, force: true })
}

main()
