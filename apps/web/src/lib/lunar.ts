/**
 * 农历 + 法定节假日（含调休 休/班）—— 纯前端本地计算，零依赖、离线可用。
 *
 * 为什么自写而不用 npm 包：npm 上的 solarlunar 实测是坏的（任意输入返回同一个"今天"），
 * 而农历/调休错了比不显示更糟。本文件的 lunarInfo 表是社区沿用二十年的标准表，
 * scripts/lunar_test.mjs 用真实锚点（历年春节/端午/中秋/闰月）+ 全-range 逐日递推一致性校验兜底。
 *
 * 表覆盖 1900-01-31 ～ 2049-12-31；超出范围返回 null（UI 自动隐藏农历行，不显示错误数据）。
 *
 * 调休数据：2025 为国务院办公厅公布口径；2026 为按惯例推算（官方公报公布后更新 HOLIDAY_DATA
 * 即可，结构不变）。表格之外的年份走「固定节日规则」兜底：能标出节日当天为休，但不含调休。
 */

// 每年 12 位压缩信息：[0..3]位=闰月月份(0=无闰) [4..15]位=13 个月大小 bit（高位在前）
const LUNAR_INFO = [
  0x04bd8, 0x04ae0, 0x0a570, 0x054d5, 0x0d260, 0x0d950, 0x16554, 0x056a0, 0x09ad0, 0x055d2, // 1900-1909
  0x04ae0, 0x0a5b6, 0x0a4d0, 0x0d250, 0x1d255, 0x0b540, 0x0d6a0, 0x0ada2, 0x095b0, 0x14977, // 1910-1919
  0x04970, 0x0a4b0, 0x0b4b5, 0x06a50, 0x06d40, 0x1ab54, 0x02b60, 0x09570, 0x052f2, 0x04970, // 1920-1929
  0x06566, 0x0d4a0, 0x0ea50, 0x16a95, 0x05ad0, 0x02b60, 0x186e3, 0x092e0, 0x1c8d7, 0x0c950, // 1930-1939
  0x0d4a0, 0x1d8a6, 0x0b550, 0x056a0, 0x1a5b4, 0x025d0, 0x092d0, 0x0d2b2, 0x0a950, 0x0b557, // 1940-1949
  0x06ca0, 0x0b550, 0x15355, 0x04da0, 0x0a5b0, 0x14573, 0x052b0, 0x0a9a8, 0x0e950, 0x06aa0, // 1950-1959
  0x0aea6, 0x0ab50, 0x04b60, 0x0aae4, 0x0a570, 0x05260, 0x0f263, 0x0d950, 0x05b57, 0x056a0, // 1960-1969
  0x096d0, 0x04dd5, 0x04ad0, 0x0a4d0, 0x0d4d4, 0x0d250, 0x0d558, 0x0b540, 0x0b6a0, 0x195a6, // 1970-1979
  0x095b0, 0x049b0, 0x0a974, 0x0a4b0, 0x0b27a, 0x06a50, 0x06d40, 0x0af46, 0x0ab60, 0x09570, // 1980-1989
  0x04af5, 0x04970, 0x064b0, 0x074a3, 0x0ea50, 0x06b58, 0x055c0, 0x0ab60, 0x096d5, 0x092e0, // 1990-1999
  0x0c960, 0x0d954, 0x0d4a0, 0x0da50, 0x07552, 0x056a0, 0x0abb7, 0x025d0, 0x092d0, 0x0cab5, // 2000-2009
  0x0a950, 0x0b4a0, 0x0baa4, 0x0ad50, 0x055d9, 0x04ba0, 0x0a5b0, 0x15176, 0x052b0, 0x0a930, // 2010-2019
  0x07954, 0x06aa0, 0x0ad50, 0x05b52, 0x04b60, 0x0a6e6, 0x0a4e0, 0x0d260, 0x0ea65, 0x0d530, // 2020-2029
  0x05aa0, 0x076a3, 0x096d0, 0x04afb, 0x04ad0, 0x0a4d0, 0x1d0b6, 0x0d250, 0x0d520, 0x0dd45, // 2030-2039
  0x0b5a0, 0x056d0, 0x055b2, 0x049b0, 0x0a577, 0x0a4b0, 0x0aa50, 0x1b255, 0x06d20, 0x0ada0, // 2040-2049
]

const LUNAR_START_YEAR = 1900
const LUNAR_END_YEAR = LUNAR_START_YEAR + LUNAR_INFO.length - 1

const MONTH_CN = ['正', '二', '三', '四', '五', '六', '七', '八', '九', '十', '冬', '腊']
const DAY_CN = [
  '初一', '初二', '初三', '初四', '初五', '初六', '初七', '初八', '初九', '初十',
  '十一', '十二', '十三', '十四', '十五', '十六', '十七', '十八', '十九', '二十',
  '廿一', '廿二', '廿三', '廿四', '廿五', '廿六', '廿七', '廿八', '廿九', '三十',
]
const ANIMALS = ['鼠', '牛', '虎', '兔', '龙', '蛇', '马', '羊', '猴', '鸡', '狗', '猪']

/** 农历日期；超出表范围（<1900-01-31 或 >2049-12-31）返回 null */
export interface LunarDate {
  year: number
  month: number // 1-12（真实月份序，闰月为其月份序）
  day: number // 1-30
  isLeap: boolean
  monthCn: string // 如「腊月」「闰六月」
  dayCn: string // 如「初五」「十五」「廿八」
  animal: string
}

function lYearDays(y: number): number {
  let sum = 348
  const info = LUNAR_INFO[y - LUNAR_START_YEAR]
  for (let b = 0x8000; b > 0x8; b >>= 1) if (info & b) sum++
  return sum + leapDays(y)
}

function leapMonth(y: number): number {
  return LUNAR_INFO[y - LUNAR_START_YEAR] & 0xf
}

function leapDays(y: number): number {
  if (leapMonth(y) === 0) return 0
  return LUNAR_INFO[y - LUNAR_START_YEAR] & 0x10000 ? 30 : 29
}

function monthDays(y: number, m: number): number {
  return LUNAR_INFO[y - LUNAR_START_YEAR] & (0x10000 >> m) ? 30 : 29
}

function parseParts(date: string): { y: number; m: number; d: number } | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(date)
  if (!m) return null
  return { y: Number(m[1]), m: Number(m[2]), d: Number(m[3]) }
}

/** 纯日历运算：与 1900-01-31 相差的天数。
 *  必须用 Date.UTC —— 1970 年前 Date 会按历史 LMT（上海 +8:05:43）解析本地时间，
 *  相邻两天的差值可能被 343 秒的时区漂移带偏 ±1 天（1919 年实测出现重复/跳日）。 */
function dayOffsetFromBase(date: string): number | null {
  const p = parseParts(date)
  if (!p) return null
  return Math.round(
    (Date.UTC(p.y, p.m - 1, p.d) - Date.UTC(1900, 0, 31)) / 86400000
  )
}

export function solar2lunar(date: string): LunarDate | null {
  const offset0 = dayOffsetFromBase(date)
  if (offset0 === null || offset0 < 0) return null
  let offset = offset0

  // 经典算法：先定位农历年（循环结束时 offset<=0，回补成当年内 0 基天数）
  let i = LUNAR_START_YEAR
  let temp = 0
  for (; i <= LUNAR_END_YEAR && offset > 0; i++) {
    temp = lYearDays(i)
    offset -= temp
  }
  if (offset < 0) {
    offset += temp
    i--
  }
  if (i < LUNAR_START_YEAR || i > LUNAR_END_YEAR) return null
  const year = i

  // 再定位农历月：闰月插在 leap 月之后（i == leap+1 时先消耗闰月天数）
  const leap = leapMonth(year)
  let isLeap = false
  let month = 1
  temp = 0
  for (; month <= 12 && offset > 0; month++) {
    if (leap > 0 && month === leap + 1 && !isLeap) {
      month--
      isLeap = true
      temp = leapDays(year)
    } else {
      temp = monthDays(year, month)
    }
    if (isLeap && month === leap + 1) isLeap = false
    offset -= temp
  }
  if (offset === 0 && leap > 0 && month === leap + 1) {
    if (isLeap) {
      isLeap = false
    } else {
      isLeap = true
      month--
    }
  }
  if (offset < 0) {
    offset += temp
    month--
  }
  if (month < 1 || month > 12) return null

  const day = offset + 1
  return {
    year,
    month,
    day,
    isLeap,
    monthCn: (isLeap ? '闰' : '') + MONTH_CN[month - 1] + '月',
    dayCn: DAY_CN[day - 1],
    animal: ANIMALS[(year - 4) % 12],
  }
}

/** 农历节日（月日匹配）；除夕单独由 holidayInfo 计算 */
const LUNAR_FESTIVALS: Record<string, string> = {
  '1-1': '春节',
  '1-15': '元宵节',
  '2-2': '龙抬头',
  '5-5': '端午节',
  '7-7': '七夕节',
  '8-15': '中秋节',
  '9-9': '重阳节',
  '12-8': '腊八节',
}

/** 法定节假日调休表。off=休假，work=补班。日期键为 MM-DD。 */
export interface YearSchedule {
  off: Record<string, string>
  work: Record<string, string>
}

export const HOLIDAY_DATA: Record<number, YearSchedule> = {
  // 2025：国务院办公厅公布口径
  2025: {
    off: {
      ...range('01-01', 1, '元旦'),
      ...range('01-28', 8, '春节'),
      ...range('04-04', 3, '清明节'),
      ...range('05-01', 5, '劳动节'),
      ...range('05-31', 3, '端午节'),
      ...range('10-01', 8, '国庆节·中秋节'),
    },
    work: { '01-26': '春节', '02-08': '春节', '04-27': '劳动节', '09-28': '国庆节', '10-11': '国庆节' },
  },
  // 2026：按惯例推算（国务院公报公布后请以官方为准更新此表）
  2026: {
    off: {
      ...range('01-01', 3, '元旦'),
      ...range('02-15', 8, '春节'),
      ...range('04-04', 3, '清明节'),
      ...range('05-01', 5, '劳动节'),
      ...range('06-19', 3, '端午节'),
      ...range('09-25', 3, '中秋节'),
      ...range('10-01', 7, '国庆节'),
    },
    work: { '02-14': '春节', '02-28': '春节', '04-26': '劳动节', '09-20': '中秋节', '10-10': '国庆节' },
  },
}

/** 生成从 MM-DD 起连续 n 天的键值 */
function range(start: string, n: number, name: string): Record<string, string> {
  const [m, d] = start.split('-').map(Number)
  const out: Record<string, string> = {}
  const cur = new Date(2001, m - 1, d) // 用闰年无关的年份即可（2 月无跨月问题）
  for (let i = 0; i < n; i++) {
    const key = `${String(cur.getMonth() + 1).padStart(2, '0')}-${String(cur.getDate()).padStart(2, '0')}`
    out[key] = name
    cur.setDate(cur.getDate() + 1)
  }
  return out
}

export interface HolidayInfo {
  /** off=法定休假（含调休连休） work=补班日 null=普通日（周末亦为 null，仅样式区分） */
  kind: 'off' | 'work' | null
  name: string
  /** true=来自调休表；false=表外年份的固定节日规则推断（不含调休） */
  scheduled: boolean
}

/** 某公历日的节假日信息（含调休 休/班） */
export function holidayInfo(date: string): HolidayInfo {
  const md = date.slice(5)
  const year = Number(date.slice(0, 4))
  const table = HOLIDAY_DATA[year]
  if (table) {
    if (table.work[md]) return { kind: 'work', name: table.work[md], scheduled: true }
    if (table.off[md]) return { kind: 'off', name: table.off[md], scheduled: true }
    return { kind: null, name: '', scheduled: true }
  }
  // 表外年份：固定规则推断（当天本身是节日则视为休，无法得知调休）
  const lun = solar2lunar(date)
  if (md === '01-01') return { kind: 'off', name: '元旦', scheduled: false }
  if (md === '05-01') return { kind: 'off', name: '劳动节', scheduled: false }
  if (md === '10-01') return { kind: 'off', name: '国庆节', scheduled: false }
  if (lun && !lun.isLeap) {
    if (lun.month === 1 && lun.day <= 3) return { kind: 'off', name: '春节', scheduled: false }
    if (lun.month === 5 && lun.day === 5) return { kind: 'off', name: '端午节', scheduled: false }
    if (lun.month === 8 && lun.day === 15) return { kind: 'off', name: '中秋节', scheduled: false }
    if (isChuxi(date, lun)) return { kind: 'off', name: '除夕', scheduled: false }
  }
  return { kind: null, name: '', scheduled: false }
}

/** 是否除夕：明天是正月初一（UTC 日历运算，理由同 dayOffsetFromBase） */
function isChuxi(date: string, lun: LunarDate): boolean {
  if (!lun || lun.month !== 12) return false
  const p = parseParts(date)
  if (!p) return false
  const next = new Date(Date.UTC(p.y, p.m - 1, p.d + 1))
  const nl = solar2lunar(fmtUTC(next))
  return !!nl && nl.month === 1 && nl.day === 1 && !nl.isLeap
}

function fmtUTC(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())}`
}

function fmtDate(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

/** 农历节日名（除夕并入）；无则空串 */
export function lunarFestival(date: string): string {
  const lun = solar2lunar(date)
  if (!lun) return ''
  if (!lun.isLeap && isChuxi(date, lun)) return '除夕'
  return LUNAR_FESTIVALS[`${lun.month}-${lun.day}`] || ''
}

/**
 * 日历格副标签：优先级 节日 > 调休名（休日名）> 农历。
 * 农历显示约定：初一显示月份（如「八月」），其余显示日（如「初九」）。
 */
export function daySubLabel(date: string): string {
  const fest = lunarFestival(date)
  if (fest) return fest
  const hol = holidayInfo(date)
  if (hol.kind === 'off' && hol.name) return hol.name.split('·')[0]
  const lun = solar2lunar(date)
  if (!lun) return ''
  return lun.day === 1 ? lun.monthCn.replace('闰', '闰') : lun.dayCn
}

/** 休/班 徽标字符：'休' | '班' | null */
export function dayBadge(date: string): '休' | '班' | null {
  const k = holidayInfo(date).kind
  return k === 'off' ? '休' : k === 'work' ? '班' : null
}
