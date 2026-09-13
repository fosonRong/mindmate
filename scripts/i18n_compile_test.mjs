/**
 * 国际化「消息编译」验收 —— 防白屏专项
 *
 * 为什么需要它（真机踩过）：
 *   vue-i18n 的消息编译器对词条文本有**自己的语法**：`@` 会被当作「链接消息」、
 *   `|` 会被当作复数分隔符、`{` 必须闭合。词条一旦踩到这些字符，`t()` 会在运行时抛
 *   SyntaxError；如果发生在模板渲染中，Vue 会把整块界面渲染成空白 —— 用户看到的就是
 *   「点这个菜单就白屏」，而且控制台只有一行 `SyntaxError: 10`，极难定位。
 *
 *   覆盖率/占位符校验（scripts/i18n_catalog.py）只看"有没有翻译"，看不到"能不能编译"，
 *   所以这里用 vue-i18n 同款的 @intlify/message-compiler 把**每一条** key 与 value
 *   都真正编译一遍，把问题拦在构建之前。
 *
 * 用法：node scripts/i18n_compile_test.mjs
 */
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
// 用 vue-i18n 内部同一套编译器（core-base）逐条编译：它抛出的 SyntaxError.code
// 与线上症状完全对应（例如 code=10 即 INVALID_LINKED_FORMAT，就是 "@" 引起的白屏）
import { compile, createCoreContext } from '@intlify/core-base'

const ROOT = path.dirname(path.dirname(fileURLToPath(import.meta.url)))
const MSG_DIR = path.join(ROOT, 'apps', 'web', 'src', 'i18n', 'messages')
const LANGS = ['zh-CN', 'en-US', 'ja-JP', 'ko-KR']

const passed = []
const failed = []
function check(name, cond, detail = '') {
  ;(cond ? passed : failed).push(name)
  console.log(`  [${cond ? 'PASS' : 'FAIL'}] ${name}${detail ? ` — ${detail}` : ''}`)
}

/** 把 TS 词条表转成可直接 import 的 ESM（表内无 import，只有结尾的 TS 断言） */
async function loadCatalog(lang, tmpDir) {
  const src = fs.readFileSync(path.join(MSG_DIR, `${lang}.ts`), 'utf8')
  const stripped = src.replace(/\}\s*as\s+Record<string,\s*string>\s*$/, '}')
  if (stripped === src) {
    throw new Error(`${lang}.ts 结构已变化：未找到结尾的 "} as Record<string, string>"，请同步本脚本`)
  }
  const file = path.join(tmpDir, `${lang}.mjs`)
  fs.writeFileSync(file, stripped, 'utf8')
  const mod = await import(pathToFileURL(file).href)
  return mod.default
}

/** 编译上下文：与运行时一致的最小上下文 */
const ctx = createCoreContext({
  locale: 'zh-CN',
  messages: { 'zh-CN': {} },
  missingWarn: false,
  fallbackWarn: false
})

/** 编译单条消息，返回 null 表示通过，否则返回错误描述 */
function compileError(msg) {
  try {
    compile(msg, ctx)
    return null
  } catch (e) {
    const code = e && e.code !== undefined ? `code=${e.code}` : ''
    return `${e && e.message ? e.message : String(e)} ${code}`.trim()
  }
}

const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mindmate-i18n-'))
let catalogs = {}
try {
  for (const lang of LANGS) catalogs[lang] = await loadCatalog(lang, tmpDir)

  console.log('\n1. 词条可编译性（key 与 value 都要能被 vue-i18n 编译）')
  let total = 0
  for (const lang of LANGS) {
    const cat = catalogs[lang]
    const keys = Object.keys(cat)
    total += keys.length
    const bad = []
    for (const k of keys) {
      const ek = compileError(k)
      const ev = compileError(cat[k])
      if (ek) bad.push(`key ${JSON.stringify(k)} → ${ek}`)
      if (ev) bad.push(`value[${JSON.stringify(k)}] → ${ev}`)
    }
    check(
      `${lang} 全部 ${keys.length} 条词条可编译`,
      bad.length === 0,
      bad.length ? `${bad.length} 条失败，示例：${bad.slice(0, 3).join(' | ')}` : '无语法错误'
    )
  }
  console.log(`  （四语合计 ${total} 条）`)

  console.log('\n2. 目录完整性')
  const sizes = LANGS.map((l) => Object.keys(catalogs[l]).length)
  check(
    '四语词条数一致',
    sizes.every((n) => n === sizes[0]),
    `zh-CN=${sizes[0]} en-US=${sizes[1]} ja-JP=${sizes[2]} ko-KR=${sizes[3]}`
  )
  check('词条规模未异常缩水（>300 条）', sizes[0] > 300, `zh-CN ${sizes[0]} 条`)

  const zhKeys = new Set(Object.keys(catalogs['zh-CN']))
  for (const lang of LANGS.slice(1)) {
    const missing = [...zhKeys].filter((k) => !(k in catalogs[lang]))
    const extra = Object.keys(catalogs[lang]).filter((k) => !zhKeys.has(k))
    check(`${lang} 与 zh-CN 键集一致`, missing.length === 0 && extra.length === 0,
      missing.length || extra.length ? `缺 ${missing.slice(0, 3).join(',')} 多 ${extra.slice(0, 3).join(',')}` : '一致')
  }

  console.log('\n3. 危险字符（历史上导致过白屏）')
  const risky = []
  for (const lang of LANGS) {
    for (const [k, v] of Object.entries(catalogs[lang])) {
      for (const [field, s] of [['key', k], ['value', v]]) {
        if (s.includes('@')) risky.push(`${lang} ${field} 含未转义 @：${s.slice(0, 40)}`)
        if (s.includes('|')) risky.push(`${lang} ${field} 含未转义 |：${s.slice(0, 40)}`)
      }
    }
  }
  check('无未转义的 @ / |（需写字面量请改用文字描述或 {..} 转义写法）', risky.length === 0,
    risky.length ? `${risky.length} 处：${risky.slice(0, 2).join(' | ')}` : '干净')
} finally {
  fs.rmSync(tmpDir, { recursive: true, force: true })
}

console.log('\n' + '='.repeat(60))
console.log(`国际化消息编译验收：通过 ${passed.length} 项，失败 ${failed.length} 项`)
for (const f of failed) console.log(`  - ${f}`)
console.log('='.repeat(60))
process.exit(failed.length ? 1 : 0)
