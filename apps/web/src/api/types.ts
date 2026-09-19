// 与 Rust 侧 serde(camelCase) 对齐的数据类型

export interface Node {
  id: number
  content: string
  date: string
  createdAt: string
  updatedAt: string
  isBackfill: boolean
  tags: string[]
  todoId: number | null
}

/** 智能速记拆出的待办（v1.1.2） */
export interface ExtractedTodo {
  title: string
  /** YYYY-MM-DD */
  date: string
  /** HH:MM 或 null（全天） */
  time: string | null
}

/** 标签统计（标签选择器数据源：记录 + 待办合并后的使用次数） */
export interface TagStat {
  name: string
  count: number
}

export interface Todo {
  id: number
  title: string
  /** 详细说明（可为空） */
  description: string
  dueDate: string
  dueTime: string | null
  remindAt: string | null
  priority: '高' | '中' | '低' | string
  tags: string[]
  status: '待处理' | '进行中' | '已完成' | '已逾期' | string
  category: '今日' | '本周' | '本月' | '日程' | string
  sortOrder: number
  createdAt: string
  updatedAt: string
  completedAt: string | null
  /** 循环类型：''=不循环 daily=每天 weekly=每周 monthly=每月 */
  recurType: string
  /** 循环锚点日期（首个实例的 dueDate） */
  recurAnchor: string
  /** 循环截止日期（空=无限） */
  recurUntil: string
  /** 周期间隔 N（每天=N 天、每周=N 周、每月=N 月） */
  recurInterval: number
  /** 落在休息日顺延到下一个工作日 */
  recurSkipRest: boolean
  /** 本实例由哪个根实例生成（用户手建的是根：null） */
  recurSourceId: number | null
  overdue: boolean
}

export interface NewsItem {
  title: string
  url: string
  hot: number | null
  channel: string
  channelName: string
}

export interface HotNewsResult {
  items: NewsItem[]
  /** live=本次实抓 cache=回退缓存 none=无数据 */
  source: string
  fetchedAt: string
  errors: string[]
  /** true=源站抓取失败后回退的历史数据 */
  stale: boolean
}

export interface NewsChannel {
  id: string
  name: string
}

/** 重点关注行业（预设，关键词由后端注册表维护） */
export interface FocusTopic {
  id: string
  name: string
}

export interface DailyStats {
  date: string
  nodeCount: number
  dailyGoal: number
  goalEnabled: boolean
  streakDays: number
  totalTodos: number
  doneTodos: number
  overdueTodos: number
  todayTodos: number
  todayDoneTodos: number
}

export interface DayStat {
  date: string
  nodeCount: number
  nodeSummaries: string[]
  totalTodos: number
  doneTodos: number
}

export interface PeriodStats {
  from: string
  to: string
  days: DayStat[]
  nodeCount: number
  daysWithRecords: number
  totalDays: number
  totalTodos: number
  doneTodos: number
}

export interface MonthlySummary {
  month: string
  nodeCount: number
  daysWithRecords: number
  totalDays: number
  doneTodos: number
  totalTodos: number
  longestStreak: number
  avgPerActiveDay: number
  isMonthEnd: boolean
}

export interface AchievementDef {
  id: string
  name: string
  description: string
  icon: string
  condition: string
  unlocked: boolean
  unlockedAt: string | null
}

export interface PushConfig {
  channels: string[]
  smtpHost: string
  smtpPort: number
  smtpUser: string
  emailFrom: string
  emailTo: string
  smtpSecurity: 'starttls' | 'tls' | 'none' | string
  hasSmtpPassword: boolean
  telegramChatId: string
  hasTelegramToken: boolean
  wecomWebhook: string
}

export interface PushResult {
  channel: string
  ok: boolean
  detail: string
}

export interface Report {
  id: number
  type: string
  period: string
  content: string
  isAi: boolean
  createdAt: string
}

export interface ChatMessage {
  id: number
  sessionId: string
  role: 'user' | 'assistant'
  content: string
  createdAt: string
}

export interface Setting {
  key: string
  value: string
}

export interface Achievement {
  id: string
  unlockedAt: string
}

export interface AiConfig {
  provider: string
  baseUrl: string
  model: string
  temperature: number
  maxTokens: number
  hasKey: boolean
  /** 协议模式：auto（默认）/ openai / anthropic */
  protocolMode: 'auto' | 'openai' | 'anthropic' | string
  /** 由地址 + 模式识别出的实际协议，用于界面提示 */
  detectedProtocol: 'openai' | 'anthropic' | string
}

export interface Preset {
  id: string
  name: string
  baseUrl: string
  /** 该提供商的 Anthropic 兼容端点（可选） */
  anthropicUrl: string
  models: string[]
  note: string
  recommended: boolean
  freeModel: string | null
  /** 申请 API Key 的官方入口（Ollama 为下载页），用于「去申请 Key」外链 */
  keyUrl: string
  /** 是否需要 API Key（Ollama 本地模型不需要） */
  requiresKey: boolean
}

/** 测试连接的失败类别（由后端 ai::classify 判定） */
export type AiFailKind =
  | 'not_configured'
  | 'auth'
  | 'quota'
  | 'endpoint'
  | 'model'
  | 'rate_limit'
  | 'upstream'
  | 'network'
  | 'parse'

/** 测试连接结论：失败也是正常结果，用 ok 表达，而不是抛错 */
export interface AiTestResult {
  ok: boolean
  kind: AiFailKind | ''
  upstreamStatus: number | null
  /** 上游返回的错误原文（便于用户/客服核对） */
  detail: string
  model: string
  latencyMs: number
  reply: string
}

/** 本机 Ollama 探测结果 */
export interface OllamaProbe {
  running: boolean
  baseUrl: string
  models: string[]
  detail: string
}

export interface AuthStatus {
  mode: string
  requiresLogin: boolean
  hasPassword: boolean
  lanEnabled: boolean
}

export interface AppEvent {
  kind: string
  payload: any
  at: string
}

export interface ScheduleData {
  date: string
  schedules: Todo[]
  todos: Todo[]
}
