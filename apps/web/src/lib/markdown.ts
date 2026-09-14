/**
 * Markdown 预处理：剥掉包裹整个内容的代码围栏。
 *
 * 真机踩过：模型偶尔会把**整份报告**包在 ```markdown … ``` 里输出，
 * 界面上就是一份等宽字体的"源码"（日报曾这样、周报正常，随模型发挥而定）。
 * 渲染前先脱围栏，同时覆盖历史报告里已落库的围栏内容，无需迁移数据。
 *
 * 只在「内容整体被围栏包住」时才剥：首行是 ``` / ```markdown / ```md，
 * 最后一行是 ``` 闭合。指定了其它语言（```js 等）视为"就要看这段代码"，原样保留。
 */
export function unwrapMarkdownFence(md: string): string {
  const t = (md || '').trim()
  if (!t.startsWith('```')) return md
  const lines = t.split('\n')
  if (!/^```(markdown|md)?$/i.test(lines[0].trim())) return md
  if (lines.length < 3) return md
  if (lines[lines.length - 1].trim() !== '```') return md
  return lines.slice(1, -1).join('\n')
}
