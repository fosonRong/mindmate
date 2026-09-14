#!/usr/bin/env node
/**
 * Markdown 渲染验收：重点防「报告被 ```markdown 围栏包住、界面显示源码」（真机踩过）。
 *
 * 直接 import 前端的同一份实现（apps/web/src/lib/markdown.ts，Node 24 原生支持 TS 类型剥离），
 * 保证测试与运行时行为一致，不存在"测试复刻一份逻辑然后漂移"的问题。
 *
 * 用法：node scripts/markdown_test.mjs
 */
import { unwrapMarkdownFence } from '../apps/web/src/lib/markdown.ts'

const passed = []
const failed = []
function check(name, cond, detail = '') {
  ;(cond ? passed : failed).push(name)
  console.log(`  [${cond ? 'PASS' : 'FAIL'}] ${name}${detail ? ` — ${detail}` : ''}`)
}

console.log('1. 整体被 ```markdown 围栏包住 → 剥掉（真机案例：日报显示源码）')
const wrapped = '```markdown\n# 2026-09-14 日报\n\n## 今日完成\n- 编写方案\n```'
check('围栏被剥掉', unwrapMarkdownFence(wrapped).startsWith('# 2026-09-14 日报'))
check('内容完整保留', unwrapMarkdownFence(wrapped).includes('- 编写方案'))
check('无围栏残留', !unwrapMarkdownFence(wrapped).includes('```'))

console.log('\n2. 围栏变体')
check('```md 语言标记同样剥离', unwrapMarkdownFence('```md\n# 标题\n```') === '# 标题')
check('无语言标记（裸 ```）同样剥离', unwrapMarkdownFence('```\n# 标题\n```') === '# 标题')
check('围栏前后有空白也能剥', unwrapMarkdownFence('\n\n```markdown\n# 标题\n```\n  ') === '# 标题')
check('多段落内容完整', (() => {
  const out = unwrapMarkdownFence('```markdown\n# A\n\n正文一\n\n## B\n\n正文二\n```')
  return out === '# A\n\n正文一\n\n## B\n\n正文二'
})())

console.log('\n3. 不该剥的不能剥（避免把代码块当正文渲染）')
check('```js 代码块原样保留', unwrapMarkdownFence('```js\nconst a = 1\n```').startsWith('```js'))
check('```python 原样保留', unwrapMarkdownFence('```python\nprint(1)\n```').startsWith('```python'))
check('普通正文原样返回', unwrapMarkdownFence('# 标题\n\n正文') === '# 标题\n\n正文')
check('只有首行围栏（未闭合/流式中途）不剥', unwrapMarkdownFence('```markdown\n# 标题') === '```markdown\n# 标题')
check('围栏未在末行闭合不剥', unwrapMarkdownFence('```markdown\n# 标题\n```\n后面还有内容').startsWith('```markdown'))

console.log('\n4. 正常报告含局部代码块不受影响')
const withInnerCode = '# 报告\n\n示例：\n\n```sql\nSELECT 1\n```\n\n完'
check('内嵌代码块保持不变', unwrapMarkdownFence(withInnerCode) === withInnerCode)

console.log('\n5. 空值与边界')
check('空字符串安全', unwrapMarkdownFence('') === '')
check('null/undefined 安全', unwrapMarkdownFence(undefined) === undefined || unwrapMarkdownFence(undefined) === '')
check('仅一个 ``` 不剥', unwrapMarkdownFence('```') === '```')
check('仅两行围栏（空内容）不剥', unwrapMarkdownFence('```markdown\n```') === '```markdown\n```')

console.log('\n' + '='.repeat(60))
console.log(`Markdown 渲染验收：通过 ${passed.length} 项，失败 ${failed.length} 项`)
for (const f of failed) console.log(`  - ${f}`)
console.log('='.repeat(60))
process.exit(failed.length ? 1 : 0)
