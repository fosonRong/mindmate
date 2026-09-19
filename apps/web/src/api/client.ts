// API 客户端：统一请求封装 + SSE 事件流（自动重连 + 断线提示）
import { t } from '@/i18n'
import type {
  Achievement, AchievementDef, AiConfig, AiTestResult, AppEvent, AuthStatus, ChatMessage, DailyStats,
  MonthlySummary, Node, OllamaProbe, PeriodStats, Preset, PushConfig, PushResult, Report,
  ScheduleData, Setting, Todo, HotNewsResult, NewsChannel, FocusTopic
} from './types'

export class ApiError extends Error {
  code: number
  constructor(code: number, message: string) {
    super(message)
    this.code = code
  }
}

let token = localStorage.getItem('mindmate_token') || ''

export function setToken(t: string) {
  token = t
  if (t) localStorage.setItem('mindmate_token', t)
  else localStorage.removeItem('mindmate_token')
}
export function getToken() {
  return token
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(init.headers as Record<string, string> | undefined)
  }
  if (token) headers['Authorization'] = `Bearer ${token}`

  const resp = await fetch(path, { ...init, headers })
  const text = await resp.text()
  let body: any = null
  try {
    body = text ? JSON.parse(text) : null
  } catch {
    throw new ApiError(5000, t('响应解析失败：{a}', { a: text.slice(0, 120) }))
  }
  if (!resp.ok || (body && body.code !== 0)) {
    throw new ApiError(body?.code ?? resp.status, body?.message || t('请求失败（{a}）', { a: resp.status }))
  }
  return body?.data as T
}

const get = <T>(p: string) => request<T>(p)
const post = <T>(p: string, data?: unknown) =>
  request<T>(p, { method: 'POST', body: JSON.stringify(data ?? {}) })
const patch = <T>(p: string, data?: unknown) =>
  request<T>(p, { method: 'PATCH', body: JSON.stringify(data ?? {}) })
const put = <T>(p: string, data?: unknown) =>
  request<T>(p, { method: 'PUT', body: JSON.stringify(data ?? {}) })
const del = <T>(p: string) => request<T>(p, { method: 'DELETE' })

export const api = {
  // 认证
  authStatus: () => get<AuthStatus>('/api/v1/auth/status'),
  login: (password: string) => post<{ token: string }>('/api/v1/auth/login', { password }),
  setPassword: (password: string) => post<{ ok: boolean }>('/api/v1/auth/set-password', { password }),

  // 记录
  nodesByDate: (date: string) => get<Node[]>(`/api/v1/nodes?date=${date}`),
  nodesRange: (from: string, to: string) => get<Node[]>(`/api/v1/nodes/range?from=${from}&to=${to}`),
  createNode: (data: { content: string; date?: string; tags?: string[]; todoId?: number | null }) =>
    post<Node>('/api/v1/nodes', data),
  updateNode: (id: number, data: { content?: string; tags?: string[]; todoId?: number | null }) =>
    patch<Node>(`/api/v1/nodes/${id}`, data),
  deleteNode: (id: number) => del<{ deleted: boolean }>(`/api/v1/nodes/${id}`),
  searchNodes: (q: string) => get<Node[]>(`/api/v1/nodes/search?q=${encodeURIComponent(q)}`),

  // 统计
  dailyStats: (date: string) => get<DailyStats>(`/api/v1/stats/daily?date=${date}`),
  periodStats: (from: string, to: string) =>
    get<PeriodStats>(`/api/v1/stats/period?from=${from}&to=${to}`),
  monthlySummary: (date: string) => get<MonthlySummary>(`/api/v1/stats/monthly?date=${date}`),

  // 待办
  todos: (params: Record<string, string> = {}) => {
    const qs = new URLSearchParams(params).toString()
    return get<Todo[]>(`/api/v1/todos${qs ? `?${qs}` : ''}`)
  },
  createTodo: (data: Partial<Todo> & { title: string; remindOffsetMin?: number }) =>
    post<Todo>('/api/v1/todos', data),
  updateTodo: (id: number, data: Record<string, unknown>) => patch<Todo>(`/api/v1/todos/${id}`, data),
  deleteTodo: (id: number, scope?: 'series') =>
    del<{ deleted: boolean; removed?: number }>(`/api/v1/todos/${id}${scope ? `?scope=${scope}` : ''}`),
  completeTodo: (id: number) => post<Todo>(`/api/v1/todos/${id}/complete`),
  reopenTodo: (id: number) => post<Todo>(`/api/v1/todos/${id}/reopen`),
  schedule: (date: string) => get<ScheduleData>(`/api/v1/todos/schedule?date=${date}`),
  suggestSchedule: (todoId: number) =>
    post<{ isAi: boolean; suggestion: string }>('/api/v1/ai/replan', { todoId }),

  // 设置
  settings: () => get<Setting[]>('/api/v1/settings'),
  updateSettings: (values: Record<string, string>) =>
    put<{ updated: number }>('/api/v1/settings', { values }),

  // 模板
  templates: () => get<{ templates: Record<string, string>; builtin: Record<string, string> }>('/api/v1/templates'),
  template: (type: string) =>
    get<{ type: string; content: string; builtin: string; customized: boolean }>(`/api/v1/templates/${type}`),
  setTemplate: (type: string, content: string) => put<{ ok: boolean }>(`/api/v1/templates/${type}`, { content }),

  // AI
  presets: () => get<Preset[]>('/api/v1/ai/presets'),
  aiConfig: () => get<AiConfig>('/api/v1/ai/config'),
  saveAiConfig: (data: {
    provider: string; baseUrl: string; model: string
    temperature: number; maxTokens: number; apiKey?: string
    protocolMode?: string
  }) => post<AiConfig>('/api/v1/ai/config', data),
  testAi: () => post<AiTestResult>('/api/v1/ai/test'),
  /** 探测本机 Ollama（11434），返回是否运行与已装模型 */
  ollamaProbe: () => get<OllamaProbe>('/api/v1/ai/ollama'),

  // 系统集成
  /** 用系统默认浏览器打开外链（仅本地模式；桌面端「去申请 Key」用） */
  openUrl: (url: string) => post<{ opened: boolean; url: string }>('/api/v1/system/open-url', { url }),

  // 今日热点（栏目清单自动生成；refresh=1 跳过 30 分钟缓存强制实抓）
  newsChannels: () => get<{ channels: NewsChannel[]; focus: FocusTopic[] }>('/api/v1/news/channels'),
  hotNews: (refresh = false, limit?: number, focus?: { topics: string[]; keywords: string[] }) => {
    const qs = new URLSearchParams()
    if (refresh) qs.set('refresh', '1')
    if (limit) qs.set('limit', String(limit))
    if (focus && focus.topics.length) qs.set('focus', focus.topics.join(','))
    if (focus && focus.keywords.length) qs.set('kw', focus.keywords.join(','))
    const q = qs.toString()
    return get<HotNewsResult>(`/api/v1/news/hot${q ? '?' + q : ''}`)
  },

  // 报告
  reports: (type?: string) => get<Report[]>(`/api/v1/reports${type ? `?type=${type}` : ''}`),
  deleteReport: (id: number) => del<{ deleted: boolean }>(`/api/v1/reports/${id}`),

  // 聊天
  chatHistory: (sessionId = 'default') => get<ChatMessage[]>(`/api/v1/chat?sessionId=${sessionId}`),
  clearChat: (sessionId = 'default') => del<{ ok: boolean }>(`/api/v1/chat?sessionId=${sessionId}`),

  // 成就（FR-6.2）
  achievements: () => get<AchievementDef[]>('/api/v1/achievements'),
  checkAchievements: () => post<AchievementDef[]>('/api/v1/achievements/check'),

  // 数据
  exportData: () => get<Record<string, unknown>>('/api/v1/data/export'),
  importData: (payload: unknown, wipe: boolean) =>
    post<{ imported: number }>('/api/v1/data/import', { payload, wipe }),

  // 推送渠道（FR-4.10）
  pushConfig: () => get<PushConfig>('/api/v1/push/config'),
  savePushConfig: (config: PushConfig, smtpPassword?: string, telegramToken?: string) =>
    post<PushConfig>('/api/v1/push/config', { config, smtpPassword, telegramToken }),
  testPush: (channel: string) => post<PushResult>('/api/v1/push/test', { channel }),

  // 健康检查
  healthz: () => get<{ status: string; version: string; mode: string }>('/api/v1/healthz')
}

/**
 * 下载文件（带鉴权头）：用于 Markdown 归档 ZIP 等二进制导出
 */
export async function downloadFile(path: string, filename: string): Promise<void> {
  const headers: Record<string, string> = {}
  if (token) headers['Authorization'] = `Bearer ${token}`
  const resp = await fetch(path, { headers })
  if (!resp.ok) {
    let msg = t('下载失败（{a}）', { a: resp.status })
    try {
      const j = await resp.json()
      if (j?.message) msg = j.message
    } catch {
      /* ignore */
    }
    throw new ApiError(resp.status, msg)
  }
  const blob = await resp.blob()
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}

/**
 * 流式接口：POST + SSE 响应解析
 * onDelta 逐段回调；返回中止函数
 */
export function streamRequest(
  path: string,
  body: unknown,
  handlers: {
    onDelta: (text: string, degraded?: boolean) => void
    onDone?: (payload: any) => void
    onError?: (message: string) => void
  }
): () => void {
  const controller = new AbortController()

  ;(async () => {
    try {
      const headers: Record<string, string> = { 'Content-Type': 'application/json' }
      if (token) headers['Authorization'] = `Bearer ${token}`
      const resp = await fetch(path, {
        method: 'POST',
        headers,
        body: JSON.stringify(body),
        signal: controller.signal
      })
      if (!resp.ok) {
        const text = await resp.text()
        try {
          const j = JSON.parse(text)
          handlers.onError?.(j.message || t('请求失败（{a}）', { a: resp.status }))
        } catch {
          handlers.onError?.(t('请求失败（{a}）', { a: resp.status }))
        }
        return
      }
      const reader = resp.body?.getReader()
      if (!reader) {
        handlers.onError?.('浏览器不支持流式响应')
        return
      }
      const decoder = new TextDecoder()
      let buffer = ''
      let eventName = ''
      while (true) {
        const { done, value } = await reader.read()
        if (done) break
        buffer += decoder.decode(value, { stream: true })
        let idx: number
        while ((idx = buffer.indexOf('\n')) >= 0) {
          const line = buffer.slice(0, idx).replace(/\r$/, '')
          buffer = buffer.slice(idx + 1)
          if (line.startsWith('event:')) {
            eventName = line.slice(6).trim()
            continue
          }
          if (!line.startsWith('data:')) continue
          const data = line.slice(5).trim()
          if (!data) continue
          try {
            const parsed = JSON.parse(data)
            if (eventName === 'delta') {
              handlers.onDelta(parsed.delta || '', parsed.degraded)
            } else if (eventName === 'done') {
              handlers.onDone?.(parsed)
            } else if (eventName === 'error') {
              handlers.onError?.(parsed.message || '生成失败')
            }
          } catch {
            /* 忽略无法解析的分片 */
          }
        }
      }
    } catch (e: any) {
      if (e?.name !== 'AbortError') handlers.onError?.(e?.message || String(e))
    }
  })()

  return () => controller.abort()
}

/** SSE 事件订阅（自动重连，指数退避） */
export function subscribeEvents(
  onEvent: (ev: AppEvent) => void,
  onStatus?: (connected: boolean) => void
): () => void {
  let es: EventSource | null = null
  let retry = 1000
  let closed = false
  const kinds = [
    'node.created', 'node.updated', 'node.deleted',
    'todo.created', 'todo.updated', 'todo.deleted', 'todo.completed',
    'report.saved', 'settings.updated', 'reminder.triggered',
    'achievement.unlocked', 'data.imported'
  ]

  const connect = () => {
    if (closed) return
    const qs = token ? `?token=${encodeURIComponent(token)}` : ''
    es = new EventSource(`/api/v1/stream/events${qs}`)

    es.addEventListener('ready', () => {
      retry = 1000
      onStatus?.(true)
    })
    for (const k of kinds) {
      es.addEventListener(k, (e: MessageEvent) => {
        try {
          onEvent(JSON.parse(e.data) as AppEvent)
        } catch {
          /* ignore */
        }
      })
    }
    es.onerror = () => {
      onStatus?.(false)
      es?.close()
      es = null
      if (closed) return
      setTimeout(connect, retry)
      retry = Math.min(retry * 2, 30000)
    }
  }

  connect()
  return () => {
    closed = true
    es?.close()
  }
}

/** SSE 流式报告（GET 场景不需要；POST 用 streamRequest） */
export const streamApi = {
  report: (type: string, date: string, handlers: Parameters<typeof streamRequest>[2]) =>
    streamRequest('/api/v1/ai/report', { type, date }, handlers),
  brief: (date: string, handlers: Parameters<typeof streamRequest>[2]) =>
    streamRequest('/api/v1/ai/brief', { date }, handlers),
  goodnight: (date: string, handlers: Parameters<typeof streamRequest>[2]) =>
    streamRequest('/api/v1/ai/goodnight', { date }, handlers),
  review: (date: string, handlers: Parameters<typeof streamRequest>[2]) =>
    streamRequest('/api/v1/ai/review', { date }, handlers),
  chat: (question: string, handlers: Parameters<typeof streamRequest>[2], sessionId = 'default') =>
    streamRequest('/api/v1/ai/chat', { question, sessionId }, handlers)
}
