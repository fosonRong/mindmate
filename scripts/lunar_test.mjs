#!/usr/bin/env node
// 农历/节假日专项验收：真实锚点 + 全范围逐日递推一致性 + 调休表结构
import { solar2lunar, holidayInfo, dayBadge, daySubLabel, lunarFestival, HOLIDAY_DATA } from '../apps/web/src/lib/lunar.ts'

let passed = 0
const failed = []
function check(name, cond, detail = '') {
  if (cond) {
    passed++
    console.log(`  [PASS] ${name}${detail ? ' — ' + detail : ''}`)
  } else {
    failed.push(name)
    console.log(`  [FAIL] ${name} — ${detail}`)
  }
}

// ── 真实锚点：农历 ──
const ANCHORS = [
  ['1900-01-31', 1, 1, false, '正月初一'],
  ['2020-01-25', 1, 1, false, '春节'],
  ['2023-06-22', 5, 5, false, '端午'],
  ['2024-02-10', 1, 1, false, '春节'],
  ['2024-09-17', 8, 15, false, '中秋'],
  ['2025-01-29', 1, 1, false, '春节'],
  ['2025-05-31', 5, 5, false, '端午'],
  ['2025-07-25', 6, 1, true, '闰六月初一'],
  ['2025-10-06', 8, 15, false, '中秋'],
  ['2026-02-17', 1, 1, false, '春节'],
  ['2027-02-06', 1, 1, false, '春节'],
]
console.log('农历锚点（历年春节/端午/中秋/闰月）')
for (const [date, m, d, leap, tag] of ANCHORS) {
  const l = solar2lunar(date)
  check(`${date} = ${tag}`, !!l && l.month === m && l.day === d && l.isLeap === leap,
    l ? `${l.monthCn}${l.dayCn}` : 'null')
}

// ── 全范围逐日递推一致性：任何相邻两天必须恰好推进一个农历日（含跨月/跨年/闰月衔接） ──
console.log('全范围逐日递推一致性（1900-01-31 → 2049-12-31，逐日推进）')
{
  const start = new Date(1900, 0, 31)
  const end = new Date(2049, 11, 31)
  let ok = true
  let badAt = ''
  let count = 0
  for (let cur = new Date(start); cur <= end; cur.setDate(cur.getDate() + 1)) {
    const key = `${cur.getFullYear()}-${String(cur.getMonth() + 1).padStart(2, '0')}-${String(cur.getDate()).padStart(2, '0')}`
    const l = solar2lunar(key)
    if (!l) { ok = false; badAt = key + ' = null'; break }
    const next = new Date(cur); next.setDate(next.getDate() + 1)
    const nl = solar2lunar(`${next.getFullYear()}-${String(next.getMonth() + 1).padStart(2, '0')}-${String(next.getDate()).padStart(2, '0')}`)
    count++
    if (!nl) {
      if (next > end) break // 范围终点合法
      ok = false; badAt = key + ' next=null'; break
    }
    // 期望：日+1；若当日为月末（day === 月长）则为下月初一
    const sameMonth = nl.year === l.year && nl.month === l.month && nl.isLeap === l.isLeap
    if (sameMonth) {
      if (nl.day !== l.day + 1) { ok = false; badAt = `${key}(${l.monthCn}${l.dayCn}) → ${nl.monthCn}${nl.dayCn}`; break }
    } else {
      if (!(nl.day === 1)) { ok = false; badAt = `${key} 跨月但次日非初一: ${nl.monthCn}${nl.dayCn}`; break }
    }
  }
  check(`逐日推进 ${count} 天无矛盾`, ok, badAt)
}

// ── 法定节假日/调休 ──
console.log('法定节假日与调休（休/班）')
check('2025 春节假期 1/28 起休 8 天', holidayInfo('2025-01-28').kind === 'off' && holidayInfo('2025-02-04').kind === 'off' && holidayInfo('2025-01-27').kind === null)
check('2025 除夕 1/28 名称含春节', holidayInfo('2025-01-28').name.includes('春节'))
check('2025 补班 2/8(周六) 标「班」', dayBadge('2025-02-08') === '班')
check('2025 补班 10/11(周六) 标「班」', dayBadge('2025-10-11') === '班')
check('2025-10-01 标「休」', dayBadge('2025-10-01') === '休')
check('2026 春节推算 2/15 起休', holidayInfo('2026-02-15').kind === 'off')
check('2026 补班 2/14 标「班」', dayBadge('2026-02-14') === '班')
check('普通周六(2026-09-19)无休/班徽标（周末靠样式区分）', dayBadge('2026-09-19') === null)
check('表外年份 2030 元旦走固定规则', holidayInfo('2030-01-01').kind === 'off' && !holidayInfo('2030-01-01').scheduled)
check('调休表至少含 2025/2026 两年且各有补班日', !!HOLIDAY_DATA[2025] && !!HOLIDAY_DATA[2026] && Object.keys(HOLIDAY_DATA[2025].work).length >= 4 && Object.keys(HOLIDAY_DATA[2026].work).length >= 3)

// ── 展示标签 ──
console.log('日历副标签')
check('2026-02-17 副标签=春节', daySubLabel('2026-02-17') === '春节')
check('2025-10-06 副标签=中秋节(农历节日优先)', daySubLabel('2025-10-06') === '中秋节')
check('2025-10-01 副标签=国庆节(调休名)', daySubLabel('2025-10-01') === '国庆节·中秋节'.split('·')[0])
check('2025-08-23(七月初一，闰六月顺延) 显示月份「七月」', daySubLabel('2025-08-23') === '七月', daySubLabel('2025-08-23'))
check('2025-07-25(闰六月初一) 显示「闰六月」', daySubLabel('2025-07-25') === '闰六月', daySubLabel('2025-07-25'))
check('2025-05-31 副标签=端午节', lunarFestival('2025-05-31') === '端午节')
check('2026-09-19 显示农历日', ['初九','初八','初十'].includes(daySubLabel('2026-09-19')), daySubLabel('2026-09-19'))
check('范围外日期(1899)安全返回空', solar2lunar('1899-12-31') === null && daySubLabel('1899-12-31') === '')
check('范围外日期(2050)安全返回空', solar2lunar('2050-06-01') === null && daySubLabel('2050-06-01') === '')

console.log(`\n农历/节假日验收：通过 ${passed} 项，失败 ${failed.length} 项`)
if (failed.length) {
  console.log('失败项：' + failed.join(' | '))
  process.exit(1)
}
