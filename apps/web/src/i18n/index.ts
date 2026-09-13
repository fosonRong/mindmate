/**
 * 国际化（中 / 英 / 日 / 韩）
 *
 * 设计取舍：**用中文原文当消息 key**（gettext 风格）。
 * 理由：现有界面已有 460+ 条中文文案，若另起一套英文 key，需要人工为每条命名并维护映射；
 * 而"原文即 key"让抽取、审阅、翻译三步都变得直接，且**缺翻译时 vue-i18n 会回退渲染 key 本身
 * （也就是中文原文）**，因此迁移过程中永远不会出现空白文案或 key 泄漏到界面上。
 *
 * - zh-CN 目录为空：中文就是 key 本身；
 * - en-US / ja-JP / ko-KR 目录给出「中文原文 → 目标语言」的映射；
 * - 持久化双写：localStorage（前端首屏即生效，含 `data-theme` 同级的预注入）+ settings 表
 *   `ui_locale`（Rust 侧据它本地化提醒文案、降级报告、成就名与 AI 提示词）。
 */

import { createI18n } from 'vue-i18n'
import enUS from './messages/en-US'
import jaJP from './messages/ja-JP'
import koKR from './messages/ko-KR'

export const SUPPORTED_LOCALES = ['zh-CN', 'en-US', 'ja-JP', 'ko-KR'] as const
export type Locale = (typeof SUPPORTED_LOCALES)[number]
export type LocaleMode = 'system' | Locale

/** 语言选择的展示名（各语言用自身文字书写，便于用户辨认） */
export const LOCALE_LABELS: Record<LocaleMode, string> = {
  system: '跟随系统',
  'zh-CN': '简体中文',
  'en-US': 'English',
  'ja-JP': '日本語',
  'ko-KR': '한국어'
}

export const LOCALE_STORAGE_KEY = 'mindmate_locale'

function isLocale(v: unknown): v is Locale {
  return typeof v === 'string' && (SUPPORTED_LOCALES as readonly string[]).includes(v)
}

/** 把各种 BCP-47 标记归一到我们支持的四种语言 */
export function normalizeLocale(tag: string | null | undefined): Locale {
  const t = (tag || '').toLowerCase()
  if (t.startsWith('zh')) return 'zh-CN'
  if (t.startsWith('ja')) return 'ja-JP'
  if (t.startsWith('ko')) return 'ko-KR'
  if (t.startsWith('en')) return 'en-US'
  // 其余语言按需求「默认跟随系统」：非支持语言统一回退英文（比回退中文更适合海外用户）
  return 'en-US'
}

/** 系统语言：浏览器与 WebView2 都提供 navigator.language，桌面端无需额外插件 */
export function systemLocale(): Locale {
  if (typeof navigator === 'undefined') return 'zh-CN'
  const cands = [...(navigator.languages || []), navigator.language].filter(Boolean) as string[]
  for (const c of cands) {
    const n = normalizeLocale(c)
    // 只有真正命中支持的语言才采用，否则继续看下一个候选
    const lower = c.toLowerCase()
    if (lower.startsWith('zh') || lower.startsWith('ja') || lower.startsWith('ko') || lower.startsWith('en')) return n
  }
  return 'zh-CN'
}

export function loadLocaleMode(): LocaleMode {
  try {
    const saved = localStorage.getItem(LOCALE_STORAGE_KEY)
    if (saved === 'system') return 'system'
    if (isLocale(saved)) return saved
  } catch {
    /* 隐私模式下忽略 */
  }
  return 'system'
}

export function resolveLocale(mode: LocaleMode): Locale {
  return mode === 'system' ? systemLocale() : mode
}

export const i18n = createI18n({
  legacy: false,
  globalInjection: true,
  locale: resolveLocale(loadLocaleMode()),
  fallbackLocale: 'zh-CN',
  messages: { 'zh-CN': {}, 'en-US': enUS, 'ja-JP': jaJP, 'ko-KR': koKR },
  // 缺翻译时静默回退，不刷控制台（由 scripts/i18n_test.py 做完整性校验）
  missingWarn: false,
  fallbackWarn: false
})

/** 当前实际生效的语言 */
export function currentLocale(): Locale {
  const l = i18n.global.locale
  return (typeof l === 'string' ? l : l.value) as Locale
}

/** 切换语言：立即生效 + 本地持久化 + 同步给 Rust（供提醒/报告/提示词使用） */
export function applyLocaleMode(mode: LocaleMode, syncToServer = true): void {
  const locale = resolveLocale(mode)
  i18n.global.locale.value = locale
  try {
    localStorage.setItem(LOCALE_STORAGE_KEY, mode)
  } catch {
    /* 忽略 */
  }
  if (typeof document !== 'undefined') document.documentElement.setAttribute('lang', locale)
  if (syncToServer) {
    // 动态 import 避免 i18n 模块与 api 客户端循环依赖
    import('@/api/client')
      .then(({ api }) => api.updateSettings({ ui_locale: locale }))
      .catch(() => {
        /* 离线/浏览器端未登录时忽略 */
      })
  }
}

/** 供组件外（stores / lib / 工具函数）使用的翻译函数 */
export function t(key: string, named?: Record<string, unknown>): string {
  return named ? (i18n.global.t as (k: string, n: Record<string, unknown>) => string)(key, named) : i18n.global.t(key)
}

/** 首屏预注入：在 Vue 挂载前把语言写到 <html lang>，避免闪烁与朗读器误判 */
export function bootstrapLocale(): void {
  if (typeof document !== 'undefined') document.documentElement.setAttribute('lang', resolveLocale(loadLocaleMode()))
}
