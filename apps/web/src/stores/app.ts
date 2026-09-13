// 应用级状态：设置、主题三态、连接状态、提醒、Toast
import { defineStore } from 'pinia'
import { api, subscribeEvents } from '@/api/client'
import type { AchievementDef, AppEvent, AuthStatus, DailyStats, Setting } from '@/api/types'

export type ThemeMode = 'system' | 'light' | 'dark'

export interface Toast {
  id: number
  kind: 'info' | 'success' | 'warning' | 'error'
  text: string
}

export interface ReminderItem {
  id: number
  kind: string
  title: string
  body: string
  action: string
  at: number
}

let toastSeq = 0
/** 事件订阅的停止函数（模块级，避免放入 Pinia state） */
let stopEvents: (() => void) | null = null

export const useAppStore = defineStore('app', {
  state: () => ({
    settings: {} as Record<string, string>,
    themeMode: 'system' as ThemeMode,
    resolvedTheme: 'light' as 'light' | 'dark',
    connected: false,
    auth: null as AuthStatus | null,
    loggedIn: true,
    stats: null as DailyStats | null,
    achievements: [] as AchievementDef[],
    toasts: [] as Toast[],
    reminders: [] as ReminderItem[],
    ready: false,
    currentDate: todayStr(),
    /** AI 是否已配置可用（用于简报/报告的降级提示） */
    aiReady: false
  }),

  getters: {
    dailyGoal: (s) => Number(s.settings.daily_goal || 4),
    goalEnabled: (s) => (s.settings.daily_goal_enabled ?? '1') === '1',
    remindFreq: (s) => Number(s.settings.remind_freq_minutes || 60),
    remindWindow: (s) => [s.settings.remind_window_start || '09:00', s.settings.remind_window_end || '21:00'],
    aiProvider: (s) => s.settings.ai_provider || 'glm',
    aiModel: (s) => s.settings.ai_model || 'glm-4-flash'
  },

  actions: {
    /** 初始化：主题、设置、认证状态、事件订阅 */
    async init() {
      this.applyTheme((localStorage.getItem('mindmate_theme') as ThemeMode) || 'system')
      try {
        this.auth = await api.authStatus()
        this.loggedIn = !this.auth.requiresLogin || !!localStorage.getItem('mindmate_token')
      } catch {
        this.loggedIn = true
      }
      if (this.loggedIn) {
        await Promise.all([this.loadSettings(), this.loadAchievements().catch(() => {})])
        await this.refreshStats()
        this.startEvents()
      }
      this.ready = true
    },

    async loadSettings() {
      const list = (await api.settings()) as Setting[]
      const map: Record<string, string> = {}
      list.forEach((s) => (map[s.key] = s.value))
      this.settings = map
      const mode = (map.theme as ThemeMode) || this.themeMode
      this.applyTheme(mode, false)
      this.refreshAiReady()
    },

    /** 刷新「AI 是否已配置」状态（配置变更后调用，避免提示滞后） */
    async refreshAiReady() {
      try {
        const cfg = await api.aiConfig()
        // ollama 无需 Key 也视为可用
        this.aiReady = !!cfg.hasKey || cfg.provider === 'ollama'
      } catch {
        this.aiReady = false
      }
    },

    async saveSettings(values: Record<string, string>) {
      await api.updateSettings(values)
      this.settings = { ...this.settings, ...values }
      if (values.theme) this.applyTheme(values.theme as ThemeMode, false)
      await this.refreshStats()
    },

    async loadAchievements() {
      this.achievements = await api.achievements()
    },

    async refreshStats() {
      try {
        this.stats = await api.dailyStats(this.currentDate)
      } catch {
        /* 离线时忽略 */
      }
    },

    // ── 主题三态 ──
    applyTheme(mode: ThemeMode, persist = true) {
      this.themeMode = mode
      if (persist) {
        localStorage.setItem('mindmate_theme', mode)
        api.updateSettings({ theme: mode }).catch(() => {})
      }
      const systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches
      this.resolvedTheme = mode === 'system' ? (systemDark ? 'dark' : 'light') : mode
      document.documentElement.setAttribute('data-theme', this.resolvedTheme)
    },

    /** 监听系统主题变化（跟随系统时实时联动） */
    watchSystemTheme() {
      window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
        if (this.themeMode === 'system') this.applyTheme('system', false)
      })
    },

    // ── 事件订阅（多端同步 + 提醒） ──
    startEvents() {
      if (stopEvents) return
      stopEvents = subscribeEvents(
        (ev: AppEvent) => this.handleEvent(ev),
        (connected) => {
          this.connected = connected
        }
      )
    },

    handleEvent(ev: AppEvent) {
      switch (ev.kind) {
        case 'reminder.triggered':
          this.pushReminder(ev.payload)
          break
        case 'achievement.unlocked':
          this.toast('success', `🏅 解锁成就：${ev.payload?.title || ev.payload?.id}`)
          this.loadAchievements().catch(() => {})
          break
        case 'settings.updated':
          this.loadSettings().catch(() => {})
          this.refreshAiReady().catch(() => {})
          break
        case 'node.created':
        case 'node.updated':
        case 'node.deleted':
        case 'todo.completed':
          this.refreshStats().catch(() => {})
          break
      }
      // 广播给页面级监听（跨端同步）
      window.dispatchEvent(new CustomEvent('mindmate:event', { detail: ev }))
    },

    pushReminder(payload: any) {
      const item: ReminderItem = {
        id: ++toastSeq,
        kind: payload.kind,
        title: payload.title,
        body: payload.body,
        action: payload.action,
        at: Date.now()
      }
      this.reminders.push(item)
      // 浏览器端系统通知（如已授权）
      tryBrowserNotification(item.title, item.body)
      // 8 秒后自动收起
      setTimeout(() => this.dismissReminder(item.id), 30000)
    },

    dismissReminder(id: number) {
      const i = this.reminders.findIndex((r) => r.id === id)
      if (i >= 0) this.reminders.splice(i, 1)
    },

    toast(kind: Toast['kind'], text: string, ms = 3000) {
      const t: Toast = { id: ++toastSeq, kind, text }
      this.toasts.push(t)
      if (this.toasts.length > 4) this.toasts.shift()
      setTimeout(() => {
        const i = this.toasts.findIndex((x) => x.id === t.id)
        if (i >= 0) this.toasts.splice(i, 1)
      }, ms)
    },

    async login(password: string) {
      const { token } = await api.login(password)
      const { setToken } = await import('@/api/client')
      setToken(token)
      this.loggedIn = true
      await this.init()
    },

    async logout() {
      const { setToken } = await import('@/api/client')
      setToken('')
      this.loggedIn = false
      stopEvents?.()
      stopEvents = null
    }
  }
})

/** 浏览器通知（授权后） */
export function tryBrowserNotification(title: string, body: string) {
  try {
    if (!('Notification' in window)) return
    if (Notification.permission === 'granted') {
      new Notification(title, { body })
    }
  } catch {
    /* 忽略 */
  }
}

export function requestNotificationPermission() {
  try {
    if ('Notification' in window && Notification.permission === 'default') {
      Notification.requestPermission()
    }
  } catch {
    /* 忽略 */
  }
}

// ── 日期工具 ──
export function todayStr(): string {
  return fmtDate(new Date())
}
export function fmtDate(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}
export function parseDate(s: string): Date {
  const [y, m, d] = s.split('-').map(Number)
  return new Date(y, (m || 1) - 1, d || 1)
}
export function addDays(s: string, n: number): string {
  const d = parseDate(s)
  d.setDate(d.getDate() + n)
  return fmtDate(d)
}
/** 周起始（周一） */
export function weekStart(s: string): string {
  const d = parseDate(s)
  const day = (d.getDay() + 6) % 7
  d.setDate(d.getDate() - day)
  return fmtDate(d)
}
export function monthRange(s: string): [string, string] {
  const d = parseDate(s)
  const start = new Date(d.getFullYear(), d.getMonth(), 1)
  const end = new Date(d.getFullYear(), d.getMonth() + 1, 0)
  return [fmtDate(start), fmtDate(end)]
}
export const WEEKDAYS = ['一', '二', '三', '四', '五', '六', '日']
export function weekdayLabel(s: string): string {
  const d = parseDate(s)
  return '周' + WEEKDAYS[(d.getDay() + 6) % 7]
}
export function friendlyDate(s: string): string {
  const d = parseDate(s)
  const t = todayStr()
  if (s === t) return '今天'
  if (s === addDays(t, 1)) return '明天'
  if (s === addDays(t, -1)) return '昨天'
  return `${d.getMonth() + 1}月${d.getDate()}日`
}
export function monthTitle(s: string): string {
  const d = parseDate(s)
  return `${d.getFullYear()} 年 ${d.getMonth() + 1} 月`
}
