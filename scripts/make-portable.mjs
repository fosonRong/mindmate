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
  const zipPath = path.join(outDir, zipName)
  const exeDir = path.join(root, 'src-tauri', 'target', 'release')
  // 用 zip 打包。注意 tar 的实现差异：Git Bash 自带的 GNU tar 会把 `D:\...` 当成远程主机
  // （报 "Cannot connect to D: resolve failed"），而 Windows 自带的 bsdtar 能正确处理盘符。
  // 所以先试 tar，失败再回退到 PowerShell 的 Compress-Archive——两条路都失败才算真失败。
  const attempts = [
    ['tar', ['-a', '-c', '-f', zipPath, '-C', exeDir, 'mindmate.exe', '-C', stage, '使用说明.txt']],
    [
      'powershell',
      [
        '-NoProfile',
        '-Command',
        `Compress-Archive -Path '${path.join(exeDir, 'mindmate.exe')}','${path.join(stage, '使用说明.txt')}' -DestinationPath '${zipPath}' -Force`
      ]
    ]
  ]
  let ok = false
  const errors = []
  for (const [cmd, args] of attempts) {
    try {
      execFileSync(cmd, args, { stdio: 'ignore' })
      ok = true
      break
    } catch (e) {
      errors.push(`${cmd}: ${e.message.split('\n')[0]}`)
    }
  }
  if (ok) {
    console.log(`✓ release/portable/${zipName}`)
  } else {
    console.warn(`⚠️  未能在本机生成绿色版 zip（tar 与 PowerShell 都失败）：${errors.join(' | ')}`)
    console.warn('   CI 上使用 windows-latest 时可正常生成；本地缺 zip 不影响发布。')
  }
  rmSync(stage, { recursive: true, force: true })
}

main()
