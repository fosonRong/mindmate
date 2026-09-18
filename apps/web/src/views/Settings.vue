<script setup lang="ts">
// 设置：提醒 / AI 模型（双预设）/ 每日目标 / 外观（主题三态）/ 数据与部署 / 关于
import { computed, onMounted, ref } from 'vue'
import { api, downloadFile } from '@/api/client'
import { useAppStore, requestNotificationPermission, type ThemeMode } from '@/stores/app'
import { isDesktop } from '@/lib/desktop'
import { useUpdateStore } from '@/stores/update'
import { LOCALE_LABELS, SUPPORTED_LOCALES, applyLocaleMode, loadLocaleMode, resolveLocale, type LocaleMode, t } from '@/i18n'
import { ref as _ref } from 'vue'
import type { AiConfig, AiFailKind, AiTestResult, OllamaProbe, Preset, PushConfig, NewsChannel } from '@/api/types'

const app = useAppStore()
const update = useUpdateStore()

type Section = 'remind' | 'push' | 'ai' | 'goal' | 'appearance' | 'data' | 'about'
const section = ref<Section>('remind')

// ── 提醒 ──
const freqMode = ref<'preset' | 'custom' | 'cron'>('preset')
const freqMinutes = ref(60)
const customMinutes = ref(90)
const cronExpr = ref('0 * * * *')
const windowStart = ref('09:00')
const windowEnd = ref('21:00')
const remindEnabled = ref(true)
const todoRemindEnabled = ref(true)
const todoRemindOffset = ref(30)
const briefEnabled = ref(true)
const briefTime = ref('09:00')
// 今日热点配置：栏目多选（清单由后端 /news/channels 自动生成）+ 显示条数
const newsChannelList = ref<NewsChannel[]>([])
const newsSelected = ref<string[]>(['weibo'])
const newsLimit = ref(10)
const newsAutoRefresh = ref(false)
const newsRefreshMinutes = ref(30)
const goodnightEnabled = ref(true)
const goodnightTime = ref('21:30')
const reviewEnabled = ref(true)
const soundEnabled = ref(true)
const dndRules = ref<{ start: string; end: string; date: string }[]>([])

// ── AI ──
const presets = ref<Preset[]>([])
const aiConfig = ref<AiConfig>({
  provider: 'glm', baseUrl: '', model: '', temperature: 0.7, maxTokens: 2048,
  hasKey: false, protocolMode: 'auto', detectedProtocol: 'openai'
})
const apiKeyInput = ref('')
const testing = ref(false)
const testResult = ref<AiTestResult | null>(null)
const savingAi = ref(false)
const showAdvanced = ref(false)
const templates = ref<Record<string, string>>({})
const editingTemplate = ref<string>('daily')
const templateVars = '{{date}} {{period}} {{nodes}} {{todos}} {{progress}} {{days}} {{context}}'
const templateDraft = ref('')
const templateSaved = ref('')

/** 当前选中的提供商（界面按它决定「要不要填 Key」等分支） */
const currentPreset = computed(() => presets.value.find((p) => p.id === aiConfig.value.provider))

/** 本机 Ollama 探测结果（本地模型用户的「一键确认能不能用」） */
const ollama = ref<OllamaProbe | null>(null)
const ollamaBusy = ref(false)

/** 模型下拉：预设模型 + 本机 Ollama 实际已装的模型（避免用户手打出错） */
const modelOptions = computed(() => {
  const base = currentPreset.value?.models || []
  const local = ollama.value?.running ? ollama.value.models : []
  return [...new Set([...base, ...local])]
})

/**
 * 失败类别 → 可操作建议（中文原文即 key，四语目录提供译文）。
 *
 * 「测试连接」的价值不在于报错，而在于告诉用户下一步改什么；
 * 类别由内核 ai::classify 依据上游状态码判定，这里只负责把它翻译成人话。
 */
const AI_HINT: Record<AiFailKind, string> = {
  not_configured: '还没填 API Key：在上方粘贴后点「保存并测试连接」',
  auth: 'Key 无效或没有权限：请确认已完整复制、没过期，并在厂商控制台确认已开通该模型',
  quota: '余额或额度不足：请到厂商控制台充值，或先换用免费模型（智谱 glm-4-flash）',
  endpoint: '接口地址不对：OpenAI 兼容地址要以 /v1 结尾，不能填网页地址（可在高级设置里恢复默认地址）',
  model: '模型名不存在：请到厂商控制台核对模型名，或从下方「模型」下拉里选一个',
  rate_limit: '请求太频繁被限流：等一两分钟再试',
  upstream: '厂商服务暂时异常：稍后重试；若一直失败，看看厂商状态页公告',
  network: '网络不通或连接超时：检查本机网络；使用 OpenAI 等境外服务通常需要开代理',
  parse: '返回内容不是接口响应：多半是 Base URL 填成了网页地址，检查一下'
}

/** 失败时的排错建议（成功或未测试时为空） */
const failHint = computed(() => {
  if (!testResult.value || testResult.value.ok) return ''
  const k = testResult.value.kind as AiFailKind
  return t(AI_HINT[k] || AI_HINT.upstream)
})

// ── 每日目标 ──
const goalEnabled = ref(true)
const dailyGoal = ref(4)

// ── 外观 ──
const themeMode = computed(() => app.themeMode)

// 界面语言：跟随系统 / 简体中文 / English / 日本語 / 한국어
const localeMode = _ref<LocaleMode>(loadLocaleMode())
const localeOptions: LocaleMode[] = ['system', ...SUPPORTED_LOCALES]
const effectiveLocale = computed(() => resolveLocale(localeMode.value))
function changeLocale(mode: LocaleMode) {
  localeMode.value = mode
  applyLocaleMode(mode) // 立即生效 + 本地持久化 + 同步给后端（提醒/报告按此语言生成）
  app.toast('success', mode === 'system' ? t('界面语言：跟随系统') : t('界面语言：{a}', { a: LOCALE_LABELS[mode] }))
}

// ── 推送渠道（FR-4.10）──
const push = ref<PushConfig>({
  channels: [], smtpHost: '', smtpPort: 587, smtpUser: '', emailFrom: '', emailTo: '',
  smtpSecurity: 'starttls', hasSmtpPassword: false, telegramChatId: '',
  hasTelegramToken: false, wecomWebhook: ''
})
const smtpPasswordInput = ref('')
const telegramTokenInput = ref('')
const pushSaving = ref(false)
const pushTesting = ref<string | null>(null)
const pushTestResult = ref<{ channel: string; ok: boolean; detail: string } | null>(null)

function channelOn(ch: string) {
  return push.value.channels.includes(ch)
}
function toggleChannel(ch: string) {
  const i = push.value.channels.indexOf(ch)
  if (i >= 0) push.value.channels.splice(i, 1)
  else push.value.channels.push(ch)
}

async function savePush() {
  pushSaving.value = true
  try {
    // 渠道启用前做最小配置校验，避免开启后静默失败
    if (channelOn('email') && (!push.value.smtpHost || !push.value.emailTo)) {
      app.toast('warning', t('启用邮件推送需要填写 SMTP 服务器与收件人'))
      return
    }
    if (channelOn('telegram') && (!push.value.telegramChatId || (!push.value.hasTelegramToken && !telegramTokenInput.value))) {
      app.toast('warning', t('启用 Telegram 需要 Chat ID 与 Bot Token'))
      return
    }
    if (channelOn('wecom') && !push.value.wecomWebhook) {
      app.toast('warning', t('启用企业微信需要 Webhook 地址'))
      return
    }
    push.value = await api.savePushConfig(push.value, smtpPasswordInput.value, telegramTokenInput.value)
    smtpPasswordInput.value = ''
    telegramTokenInput.value = ''
    app.toast('success', t('推送渠道已保存'))
  } catch (e: any) {
    app.toast('error', e?.message || '保存失败')
  } finally {
    pushSaving.value = false
  }
}

async function testPush(channel: string) {
  pushTesting.value = channel
  pushTestResult.value = null
  try {
    // 先保存当前表单再测试，确保测的是最新配置
    push.value = await api.savePushConfig(push.value, smtpPasswordInput.value, telegramTokenInput.value)
    smtpPasswordInput.value = ''
    telegramTokenInput.value = ''
    const r = await api.testPush(channel)
    pushTestResult.value = { channel, ok: r.ok, detail: r.detail }
    app.toast(r.ok ? 'success' : 'error', r.ok ? t('{a} 推送成功', { a: channel }) : t('{a} 推送失败', { a: channel }))
  } catch (e: any) {
    pushTestResult.value = { channel, ok: false, detail: e?.message || '测试失败' }
    app.toast('error', e?.message || '测试失败')
  } finally {
    pushTesting.value = null
  }
}

const CHANNEL_META: { id: string; name: string; desc: string; icon: string }[] = [
  { id: 'wecom', name: '企业微信机器人', desc: '群机器人 Webhook，最省事（推荐）', icon: '💬' },
  { id: 'email', name: '邮件', desc: 'SMTP 发送（支持 SSL / STARTTLS）', icon: '📧' },
  { id: 'telegram', name: 'Telegram', desc: 'Bot API 推送到指定会话', icon: '✈️' }
]

// ── 开机自启（桌面端）──
const isDesktopApp = isDesktop()
const autostart = ref(false)
const autostartBusy = ref(false)

async function loadAutostart() {
  if (!isDesktopApp) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    autostart.value = (await invoke<boolean>('autostart_status')) === true
  } catch (e) {
    // 读取失败不打断页面，仅记录（点击开关时会再报明确错误）
    console.warn('读取开机自启状态失败', e)
  }
}

async function toggleAutostart() {
  if (!isDesktopApp || autostartBusy.value) return
  autostartBusy.value = true
  const target = !autostart.value
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const now = await invoke<boolean>('autostart_set', { enabled: target })
    autostart.value = now === true
    if (autostart.value === target) {
      app.toast(target ? 'success' : 'info', target
        ? '已开启开机自启（登录后自动启动智伴，提醒持续生效）'
        : '已关闭开机自启')
    } else {
      app.toast('warning', t('设置未生效，请检查系统安全软件是否拦截了启动项写入'))
    }
  } catch (e: any) {
    // Tauri 命令返回的错误是字符串，给出可读提示
    const msg = typeof e === 'string' ? e : e?.message || String(e)
    app.toast('error', msg.includes('开启') || msg.includes('关闭') ? msg : t('设置开机自启失败：{a}', { a: msg }))
  } finally {
    autostartBusy.value = false
  }
}

// ── 数据 ──
const exportText = ref('')
const importText = ref('')
const wipeOnImport = ref(false)
const deployMode = ref('local')
const lanEnabled = ref(false)
const password = ref('')
const savingPassword = ref(false)

function minutesToHHMM(m: number) {
  const h = Math.floor(m / 60)
  const mm = m % 60
  return `${String(h).padStart(2, '0')}:${String(mm).padStart(2, '0')}`
}
function hhmmToMinutes(s: string) {
  const [h, m] = s.split(':').map(Number)
  return h * 60 + (m || 0)
}

async function load() {
  const s = app.settings
  freqMinutes.value = Number(s.remind_freq_minutes || 60)
  if (![60, 120, 240, 30].includes(freqMinutes.value)) {
    freqMode.value = 'custom'
    customMinutes.value = freqMinutes.value
  }
  windowStart.value = s.remind_window_start || '09:00'
  windowEnd.value = s.remind_window_end || '21:00'
  remindEnabled.value = (s.remind_enabled ?? '1') === '1'
  todoRemindEnabled.value = (s.todo_remind_enabled ?? '1') === '1'
  todoRemindOffset.value = Number(s.todo_remind_offset_min || 30)
  briefEnabled.value = (s.smart_brief_enabled ?? '1') === '1'
  briefTime.value = minutesToHHMM(Number(s.brief_minutes || 540))
  // 今日热点：条数 + 栏目（多选）+ 自动更新
  newsLimit.value = Number(s.news_limit || 10)
  newsAutoRefresh.value = (s.news_auto_refresh ?? '0') === '1'
  newsRefreshMinutes.value = Number(s.news_refresh_minutes || 30)
  try {
    const sel = JSON.parse(s.news_channels || '["weibo"]')
    newsSelected.value = Array.isArray(sel) && sel.length ? sel : ['weibo']
  } catch {
    newsSelected.value = ['weibo']
  }
  goodnightEnabled.value = (s.goodnight_enabled ?? '1') === '1'
  goodnightTime.value = minutesToHHMM(Number(s.goodnight_minutes || 1290))
  reviewEnabled.value = (s.review_enabled ?? '1') === '1'
  soundEnabled.value = (s.remind_sound ?? '1') === '1'
  goalEnabled.value = (s.daily_goal_enabled ?? '1') === '1'
  dailyGoal.value = Number(s.daily_goal || 4)
  deployMode.value = s.deploy_mode || 'local'
  try {
    dndRules.value = JSON.parse(s.dnd_rules || '[]')
  } catch {
    dndRules.value = []
  }
  // 并行拉取：原先 5 次串行往返（预设 → AI 配置 → 推送配置 → 自启状态 → 模板），
  // 每次都要等上一个回来，进设置页要等好几轮；并行后总耗时≈最慢的那一个。
  const [pres, ai, pushCfg, tpl, channels] = await Promise.all([
    api.presets(),
    api.aiConfig(),
    api.pushConfig(),
    api.templates(),
    api.newsChannels().catch(() => ({ channels: [] as NewsChannel[] }))
  ])
  presets.value = pres
  aiConfig.value = ai
  push.value = pushCfg
  templates.value = tpl.templates
  newsChannelList.value = channels.channels
  templateDraft.value = tpl.templates[editingTemplate.value] || ''
  // 自启状态不 await：它只影响一个开关，且桌面端要走一次 IPC（内核要读注册表）。
  // 让它在后台填充，页面无需等它。
  loadAutostart()
}

function toggleNewsChannel(id: string) {
  const i = newsSelected.value.indexOf(id)
  if (i >= 0) {
    if (newsSelected.value.length === 1) {
      app.toast('warning', t('至少保留一个栏目'))
      return
    }
    newsSelected.value.splice(i, 1)
  } else {
    newsSelected.value.push(id)
  }
}

async function saveRemind() {
  const freq = freqMode.value === 'custom' ? customMinutes.value : freqMinutes.value
  await app.saveSettings({
    remind_freq_minutes: String(freq),
    remind_window_start: windowStart.value,
    remind_window_end: windowEnd.value,
    remind_enabled: remindEnabled.value ? '1' : '0',
    todo_remind_enabled: todoRemindEnabled.value ? '1' : '0',
    todo_remind_offset_min: String(todoRemindOffset.value),
    smart_brief_enabled: briefEnabled.value ? '1' : '0',
    brief_minutes: String(hhmmToMinutes(briefTime.value)),
    news_limit: String(newsLimit.value),
    news_channels: JSON.stringify(newsSelected.value.length ? newsSelected.value : ['weibo']),
    news_auto_refresh: newsAutoRefresh.value ? '1' : '0',
    news_refresh_minutes: String(newsRefreshMinutes.value),
    goodnight_enabled: goodnightEnabled.value ? '1' : '0',
    goodnight_minutes: String(hhmmToMinutes(goodnightTime.value)),
    review_enabled: reviewEnabled.value ? '1' : '0',
    remind_sound: soundEnabled.value ? '1' : '0',
    dnd_rules: JSON.stringify(dndRules.value)
  })
  app.toast('success', t('提醒设置已保存'))
}

function addDnd() {
  dndRules.value.push({ start: '12:00', end: '13:00', date: '' })
}
function removeDnd(i: number) {
  dndRules.value.splice(i, 1)
}

/// 是否为任一预设的默认地址（用于判断用户是否自定义过）
function isPresetDefaultUrl(url: string) {
  const u = (url || '').trim().replace(/\/$/, '')
  if (!u) return true
  return presets.value.some(
    (x) => x.baseUrl.replace(/\/$/, '') === u || (x.anthropicUrl || '').replace(/\/$/, '') === u
  )
}

function pickPreset(p: Preset) {
  aiConfig.value.provider = p.id
  aiConfig.value.model = p.models[0]
  // 关键修复：仅当当前地址为空或仍是某个预设默认值时，才填入该预设默认地址；
  // 用户自定义过的地址不会被切换提供商/模型覆盖
  if (isPresetDefaultUrl(aiConfig.value.baseUrl)) {
    aiConfig.value.baseUrl = p.baseUrl
  }
  testResult.value = null
}

/// 显式恢复当前提供商的默认（OpenAI 兼容）地址
function useDefaultUrl() {
  const p = presets.value.find((x) => x.id === aiConfig.value.provider)
  if (p) {
    aiConfig.value.baseUrl = p.baseUrl
    testResult.value = null
    app.toast('info', t('已填入该提供商的默认 OpenAI 兼容地址'))
  }
}

/// 切换到该提供商的 Anthropic 兼容地址
function useAnthropicUrl() {
  const p = presets.value.find((x) => x.id === aiConfig.value.provider)
  if (!p?.anthropicUrl) {
    app.toast('warning', t('该提供商未提供 Anthropic 兼容地址，可手动填写'))
    return
  }
  aiConfig.value.baseUrl = p.anthropicUrl
  aiConfig.value.protocolMode = 'auto'
  testResult.value = null
  app.toast('info', t('已填入 Anthropic 兼容地址（将使用 /v1/messages 协议）'))
}

const baseUrlCustomized = computed(
  () => !!aiConfig.value.baseUrl && !isPresetDefaultUrl(aiConfig.value.baseUrl)
)

async function saveAi() {
  savingAi.value = true
  try {
    aiConfig.value = await api.saveAiConfig({
      provider: aiConfig.value.provider,
      baseUrl: aiConfig.value.baseUrl.trim(),
      model: aiConfig.value.model.trim(),
      temperature: aiConfig.value.temperature,
      maxTokens: aiConfig.value.maxTokens,
      protocolMode: aiConfig.value.protocolMode || 'auto',
      apiKey: apiKeyInput.value || undefined
    })
    apiKeyInput.value = ''
    app.toast('success', t('AI 配置已保存'))
    app.loadSettings()
    app.refreshAiReady()
  } catch (e: any) {
    app.toast('error', e?.message || '保存失败')
  } finally {
    savingAi.value = false
  }
}

/// 打开厂商的「申请 Key」页面：桌面端交给本机内核调起系统浏览器，浏览器端直接开新标签
async function openKeyPage() {
  const url = currentPreset.value?.keyUrl
  if (!url) return
  if (isDesktop()) {
    try {
      await api.openUrl(url)
      return
    } catch (e: any) {
      app.toast('warning', t('打开链接失败，请手动复制到浏览器：{a}', { a: url }))
      return
    }
  }
  window.open(url, '_blank', 'noopener')
}

/// 一键检测本机 Ollama（11434），顺带把模型切到本机已装的那个
async function probeOllama() {
  ollamaBusy.value = true
  try {
    const r = await api.ollamaProbe()
    ollama.value = r
    if (r.running) {
      if (r.models.length && !r.models.includes(aiConfig.value.model)) {
        aiConfig.value.model = r.models[0]
      }
      app.toast('success', t('已检测到本机 Ollama（{a} 个模型）', { a: r.models.length }))
    } else {
      app.toast('warning', t('未检测到本机 Ollama，请确认已安装并启动（默认端口 11434）'))
    }
  } catch (e: any) {
    app.toast('error', e?.message || t('检测失败'))
  } finally {
    ollamaBusy.value = false
  }
}

async function testConnection() {
  testing.value = true
  testResult.value = null
  try {
    // 先保存再测试，保证用最新配置
    await api.saveAiConfig({
      provider: aiConfig.value.provider,
      baseUrl: aiConfig.value.baseUrl.trim(),
      model: aiConfig.value.model.trim(),
      temperature: aiConfig.value.temperature,
      maxTokens: aiConfig.value.maxTokens,
      protocolMode: aiConfig.value.protocolMode || 'auto',
      apiKey: apiKeyInput.value || undefined
    })
    apiKeyInput.value = ''
    app.refreshAiReady()
    const r = await api.testAi()
    testResult.value = r
    if (r.ok) {
      aiConfig.value.hasKey = true
      app.toast('success', t('连接成功'))
    }
  } catch (e: any) {
    // 连内核请求都没发出去（服务未起、网络异常）：归到网络类，给出同样的排错建议
    testResult.value = {
      ok: false,
      kind: 'network',
      upstreamStatus: null,
      detail: e?.message || String(e),
      model: aiConfig.value.model,
      latencyMs: 0,
      reply: ''
    }
  } finally {
    testing.value = false
  }
}

async function loadTemplate(type: string) {
  editingTemplate.value = type
  const tpl = await api.template(type)
  templateDraft.value = tpl.content
  templateSaved.value = tpl.customized ? '已自定义' : '默认模板'
}

async function saveTemplate() {
  await api.setTemplate(editingTemplate.value, templateDraft.value)
  templateSaved.value = '已自定义'
  app.toast('success', t('模板已保存'))
}

async function resetTemplate() {
  const tpl = await api.template(editingTemplate.value)
  templateDraft.value = tpl.builtin
  await api.setTemplate(editingTemplate.value, tpl.builtin)
  templateSaved.value = '默认模板'
  app.toast('info', t('已恢复默认模板'))
}

async function saveGoal() {
  await app.saveSettings({
    daily_goal: String(dailyGoal.value),
    daily_goal_enabled: goalEnabled.value ? '1' : '0'
  })
  app.toast('success', t('每日目标已保存'))
}

async function doExport() {
  const data = await api.exportData()
  exportText.value = JSON.stringify(data, null, 2)
  const blob = new Blob([exportText.value], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `mindmate-backup-${new Date().toISOString().slice(0, 10)}.json`
  a.click()
  URL.revokeObjectURL(url)
  app.toast('success', t('备份已导出'))
}

const exportingMd = ref(false)

async function doExportMarkdown() {
  exportingMd.value = true
  try {
    await downloadFile(
      '/api/v1/data/export/markdown',
      `mindmate-archive-${new Date().toISOString().slice(0, 10)}.zip`
    )
    app.toast('success', t('Markdown 归档已导出（按日/周/月分文件）'))
  } catch (e: any) {
    app.toast('error', e?.message || '导出失败')
  } finally {
    exportingMd.value = false
  }
}

async function doImport() {
  if (!importText.value.trim()) {
    app.toast('warning', t('请粘贴备份 JSON'))
    return
  }
  try {
    const payload = JSON.parse(importText.value)
    const r = await api.importData(payload, wipeOnImport.value)
    app.toast('success', t('已导入 {a} 条数据', { a: r.imported }))
    app.loadSettings()
  } catch (e: any) {
    app.toast('error', e?.message || '导入失败，请检查 JSON 格式')
  }
}

async function savePassword() {
  if (password.value.length < 6) {
    app.toast('warning', t('密码至少 6 位'))
    return
  }
  savingPassword.value = true
  try {
    await api.setPassword(password.value)
    password.value = ''
    app.toast('success', t('访问密码已设置'))
  } catch (e: any) {
    app.toast('error', e?.message || '设置失败')
  } finally {
    savingPassword.value = false
  }
}

const THEMES: { key: ThemeMode; label: string; desc: string }[] = [
  { key: 'system', label: '跟随系统', desc: '随系统浅色/深色自动切换' },
  { key: 'light', label: '浅色', desc: '始终使用浅色主题' },
  { key: 'dark', label: '深色', desc: '始终使用深色主题' }
]

onMounted(load)
</script>

<template>
  <div class="set-split">
    <aside class="set-menu">
      <button :class="{ on: section === 'remind' }" @click="section = 'remind'">{{ $t('⏰ 提醒') }}</button>
      <button :class="{ on: section === 'push' }" @click="section = 'push'">{{ $t('📮 推送渠道') }}</button>
      <button :class="{ on: section === 'ai' }" @click="section = 'ai'">{{ $t('🤖 AI 模型') }}</button>
      <button :class="{ on: section === 'goal' }" @click="section = 'goal'">{{ $t('🎯 每日目标') }}</button>
      <button :class="{ on: section === 'appearance' }" @click="section = 'appearance'">{{ $t('🎨 外观') }}</button>
      <button :class="{ on: section === 'data' }" @click="section = 'data'">{{ $t('💾 数据与部署') }}</button>
      <button :class="{ on: section === 'about' }" @click="section = 'about'">{{ $t('ℹ️ 关于') }}</button>
    </aside>

    <div class="col-stack">
      <!-- 提醒 -->
      <template v-if="section === 'remind'">
        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('记录提醒') }}</div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">{{ $t('启用记录提醒') }}</span>
            <div class="switch" :class="{ on: remindEnabled }" @click="remindEnabled = !remindEnabled"></div>
          </div>
          <div class="form-row">
            <label class="form-label">{{ $t('提醒频率') }}</label>
            <div class="seg">
              <button :class="{ on: freqMode === 'preset' && freqMinutes === 60 }" @click="freqMode = 'preset'; freqMinutes = 60">{{ $t('每小时') }}</button>
              <button :class="{ on: freqMode === 'preset' && freqMinutes === 120 }" @click="freqMode = 'preset'; freqMinutes = 120">{{ $t('每 2 小时') }}</button>
              <button :class="{ on: freqMode === 'preset' && freqMinutes === 240 }" @click="freqMode = 'preset'; freqMinutes = 240">{{ $t('每 4 小时') }}</button>
              <button :class="{ on: freqMode === 'custom' }" @click="freqMode = 'custom'">{{ $t('自定义') }}</button>
            </div>
            <div v-if="freqMode === 'custom'" class="row" style="margin-top: 8px">
              <input v-model.number="customMinutes" type="number" min="5" max="720" class="input" style="width: 120px" />
              <span class="small muted">{{ $t('分钟提醒一次') }}</span>
            </div>
          </div>
          <div class="form-row">
            <label class="form-label">{{ $t('生效时段（时段外静默）') }}</label>
            <div class="row">
              <input v-model="windowStart" type="time" class="input" style="width: 130px" />
              <span class="muted">—</span>
              <input v-model="windowEnd" type="time" class="input" style="width: 130px" />
            </div>
          </div>
          <div class="hint-bar info">
            {{ $t('🧠 智能规则：当日记录达标即静默；有逾期待办会附加提醒；连续多日未记录改为温和关怀') }}
          </div>
        </section>

        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('待办提醒') }}</div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">{{ $t('启用待办到期提醒') }}</span>
            <div class="switch" :class="{ on: todoRemindEnabled }" @click="todoRemindEnabled = !todoRemindEnabled"></div>
          </div>
          <div class="form-row">
            <label class="form-label">{{ $t('默认提前量') }}</label>
            <div class="select-wrap" style="width: 180px">
              <select v-model.number="todoRemindOffset" class="input">
                <option :value="0">{{ $t('准点提醒') }}</option>
                <option :value="5">{{ $t('提前 5 分钟') }}</option>
                <option :value="15">{{ $t('提前 15 分钟') }}</option>
                <option :value="30">{{ $t('提前 30 分钟') }}</option>
                <option :value="60">{{ $t('提前 1 小时') }}</option>
              </select>
            </div>
          </div>
        </section>

        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('智伴主动问候') }}</div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">{{ $t('我的简报') }}</span>
            <input v-model="briefTime" type="time" class="input" style="width: 120px" />
            <div class="switch" :class="{ on: briefEnabled }" @click="briefEnabled = !briefEnabled"></div>
          </div>
          <div class="stack" style="gap: 6px">
            <div class="row">
              <span style="flex: 1; font-size: 13px">{{ $t('今日热点') }}</span>
              <div class="select-wrap" style="width: 130px">
                <select v-model.number="newsLimit" class="input">
                  <option :value="5">{{ $t('显示 5 条') }}</option>
                  <option :value="10">{{ $t('显示 10 条') }}</option>
                  <option :value="15">{{ $t('显示 15 条') }}</option>
                  <option :value="20">{{ $t('显示 20 条') }}</option>
                </select>
              </div>
            </div>
            <div class="row">
              <span style="flex: 1; font-size: 13px">{{ $t('自动更新热点') }}</span>
              <div class="select-wrap" v-show="newsAutoRefresh" style="width: 150px">
                <select v-model.number="newsRefreshMinutes" class="input">
                  <option :value="5">{{ $t('每 5 分钟') }}</option>
                  <option :value="10">{{ $t('每 10 分钟') }}</option>
                  <option :value="15">{{ $t('每 15 分钟') }}</option>
                  <option :value="30">{{ $t('每 30 分钟') }}</option>
                  <option :value="60">{{ $t('每 1 小时') }}</option>
                </select>
              </div>
              <div class="switch" :class="{ on: newsAutoRefresh }" @click="newsAutoRefresh = !newsAutoRefresh"></div>
            </div>
            <div class="row wrap" style="gap: 6px; padding-left: 2px">
              <button
                v-for="c in newsChannelList"
                :key="c.id"
                class="tag-pick"
                :class="{ on: newsSelected.includes(c.id) }"
                @click="toggleNewsChannel(c.id)"
              >
                {{ c.name }}
              </button>
              <span v-if="!newsChannelList.length" class="small muted">{{ $t('栏目清单加载失败，保存后将只显示微博热搜') }}</span>
            </div>
            <div class="small muted">{{ $t('栏目多选；在「今日」页点简报旁的「今日热点」标签查看，点击新闻用浏览器打开。') }}</div>
          </div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">{{ $t('晚安总结') }}</span>
            <input v-model="goodnightTime" type="time" class="input" style="width: 120px" />
            <div class="switch" :class="{ on: goodnightEnabled }" @click="goodnightEnabled = !goodnightEnabled"></div>
          </div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">{{ $t('周度智能复盘') }}</span>
            <div class="switch" :class="{ on: reviewEnabled }" @click="reviewEnabled = !reviewEnabled"></div>
          </div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">{{ $t('提醒提示音') }}</span>
            <div class="switch" :class="{ on: soundEnabled }" @click="soundEnabled = !soundEnabled"></div>
          </div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">{{ $t('浏览器通知权限') }}</span>
            <button class="btn btn-sm" @click="requestNotificationPermission(); app.toast('info', t('已请求通知权限（若浏览器弹窗请允许）'))">{{ $t('请求权限') }}</button>
          </div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">
              {{ $t('开机自启') }}
              <span class="small muted">{{ $t('（{a}）', { a: isDesktopApp ? $t('登录后自动启动，保证提醒不中断') : $t('仅桌面端可用') }) }}</span>
            </span>
            <div
              class="switch"
              :class="{ on: autostart }"
              :style="!isDesktopApp ? 'opacity:.5;cursor:not-allowed' : ''"
              :title="isDesktopApp ? '' : '浏览器端不支持'"
              @click="toggleAutostart"
            ></div>
          </div>
        </section>

        <section class="card stack">
          <div class="row">
            <div class="card-title" style="font-size: 15px">{{ $t('勿扰时段') }}</div>
            <div class="spacer"></div>
            <button class="btn btn-sm" @click="addDnd">{{ $t('＋ 添加') }}</button>
          </div>
          <div class="small muted">{{ $t('勿扰时段内完全静默（优先级最高），可设置一次性（指定日期）或每天重复') }}</div>
          <div v-for="(r, i) in dndRules" :key="i" class="row">
            <input v-model="r.start" type="time" class="input" style="width: 120px" />
            <span class="muted">—</span>
            <input v-model="r.end" type="time" class="input" style="width: 120px" />
            <input v-model="r.date" type="date" class="input" style="width: 150px" :placeholder="$t('限某天（可空）')" />
            <button class="icon-btn" @click="removeDnd(i)">×</button>
          </div>
        </section>

        <div class="row">
          <div class="spacer"></div>
          <button class="btn btn-primary" @click="saveRemind">{{ $t('保存提醒设置') }}</button>
        </div>
      </template>

      <!-- 推送渠道（FR-4.10）-->
      <template v-if="section === 'push'">
        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('远程推送渠道') }}</div>
          <div class="small muted">
            {{ $t('除桌面通知与浏览器横幅外，还可通过邮件或 IM 接收提醒 —— 应用未打开、或处于服务器模式时同样有效。 凭据仅保存于本机系统安全存储。') }}
          </div>
          <div v-for="c in CHANNEL_META" :key="c.id" class="channel-row" :class="{ on: channelOn(c.id) }">
            <div style="font-size: 20px; width: 28px; text-align: center">{{ c.icon }}</div>
            <div class="info">
              <div class="name">
                {{ c.name }}
                <span v-if="c.id === 'wecom'" class="badge info">{{ $t('推荐') }}</span>
                <span
                  v-if="(c.id === 'email' && push.hasSmtpPassword) || (c.id === 'telegram' && push.hasTelegramToken)"
                  class="badge ok"
                >{{ $t('已配置凭据') }}</span>
              </div>
              <div class="desc">{{ c.desc }}</div>
            </div>
            <button class="btn btn-sm" :disabled="pushTesting === c.id" @click="testPush(c.id)">
              {{ pushTesting === c.id ? $t('测试中…') : $t('测试') }}
            </button>
            <div class="switch" :class="{ on: channelOn(c.id) }" @click="toggleChannel(c.id)"></div>
          </div>
          <div
            v-if="pushTestResult"
            class="small"
            :style="pushTestResult.ok ? 'color:var(--success)' : 'color:var(--danger)'"
          >
            {{ pushTestResult.ok ? '✓' : '✗' }} {{ pushTestResult.channel }}：{{ pushTestResult.detail }}
          </div>
        </section>

        <section class="card stack" :class="{ dim: !channelOn('wecom') }">
          <div class="card-title" style="font-size: 15px">{{ $t('企业微信群机器人') }}</div>
          <div class="form-row">
            <label class="form-label">{{ $t('Webhook 地址') }}</label>
            <input
              v-model="push.wecomWebhook"
              class="input mono"
              placeholder="https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=…"
            />
          </div>
          <div class="small muted">{{ $t('群聊 → 右上角 → 群机器人 → 添加机器人 → 复制 Webhook 地址') }}</div>
        </section>

        <section class="card stack" :class="{ dim: !channelOn('email') }">
          <div class="card-title" style="font-size: 15px">{{ $t('邮件（SMTP）') }}</div>
          <div class="row" style="gap: 12px">
            <div class="form-row" style="flex: 2">
              <label class="form-label">{{ $t('SMTP 服务器') }}</label>
              <input v-model="push.smtpHost" class="input mono" placeholder="smtp.example.com" />
            </div>
            <div class="form-row" style="flex: 1">
              <label class="form-label">{{ $t('端口') }}</label>
              <input v-model.number="push.smtpPort" type="number" class="input" />
            </div>
            <div class="form-row" style="flex: 1">
              <label class="form-label">{{ $t('加密方式') }}</label>
              <div class="select-wrap" style="width: 100%">
                <select v-model="push.smtpSecurity" class="input">
                  <option value="starttls">STARTTLS (587)</option>
                  <option value="tls">SSL/TLS (465)</option>
                  <option value="none">{{ $t('无加密') }}</option>
                </select>
              </div>
            </div>
          </div>
          <div class="row" style="gap: 12px">
            <div class="form-row" style="flex: 1">
              <label class="form-label">{{ $t('发件邮箱') }}</label>
              <input v-model="push.emailFrom" class="input" placeholder="me@example.com" />
            </div>
            <div class="form-row" style="flex: 1">
              <label class="form-label">{{ $t('收件邮箱') }}</label>
              <input v-model="push.emailTo" class="input" placeholder="to@example.com" />
            </div>
          </div>
          <div class="row" style="gap: 12px">
            <div class="form-row" style="flex: 1">
              <label class="form-label">{{ $t('SMTP 账号') }}</label>
              <input v-model="push.smtpUser" class="input" :placeholder="$t('通常与发件邮箱相同')" />
            </div>
            <div class="form-row" style="flex: 1">
              <label class="form-label">
                {{ $t('密码 / 授权码') }}
                <span class="muted small">{{ $t('（{a}）', { a: push.hasSmtpPassword ? $t('已配置，留空则不修改') : $t('仅存系统安全存储') }) }}</span>
              </label>
              <input v-model="smtpPasswordInput" type="password" class="input" placeholder="••••••••" />
            </div>
          </div>
          <div class="small muted">{{ $t('企业邮箱 / QQ 邮箱等通常需在邮箱设置中开启 SMTP 并使用「授权码」而非登录密码') }}</div>
        </section>

        <section class="card stack" :class="{ dim: !channelOn('telegram') }">
          <div class="card-title" style="font-size: 15px">Telegram</div>
          <div class="row" style="gap: 12px">
            <div class="form-row" style="flex: 1">
              <label class="form-label">Chat ID</label>
              <input v-model="push.telegramChatId" class="input mono" placeholder="-1001234567890" />
            </div>
            <div class="form-row" style="flex: 1">
              <label class="form-label">
                Bot Token
                <span class="muted small">{{ $t('（{a}）', { a: push.hasTelegramToken ? $t('已配置，留空则不修改') : $t('仅存系统安全存储') }) }}</span>
              </label>
              <input v-model="telegramTokenInput" type="password" class="input mono" placeholder="123456:ABC-DEF…" />
            </div>
          </div>
          <div class="small muted">{{ $t('在 Telegram 里搜索 BotFather 创建机器人获取 Token；给机器人发消息后用 userinfobot 获取 Chat ID') }}</div>
        </section>

        <div class="row">
          <div class="spacer"></div>
          <button class="btn btn-primary" :disabled="pushSaving" @click="savePush">
            {{ pushSaving ? $t('保存中…') : $t('保存推送渠道') }}
          </button>
        </div>
      </template>

      <!-- AI 模型（T1.6：三步引导 —— 选厂商 → 领 Key → 粘贴并测试）-->
      <template v-if="section === 'ai'">
        <section class="card stack">
          <div class="row">
            <div class="card-title" style="font-size: 15px">{{ $t('第一步：选择模型提供商') }}</div>
            <div class="spacer"></div>
            <span v-if="aiConfig.hasKey" class="badge ok">{{ $t('已配置') }}</span>
          </div>
          <div class="preset-grid">
            <div
              v-for="p in presets"
              :key="p.id"
              class="preset-card"
              :class="{ on: aiConfig.provider === p.id }"
              @click="pickPreset(p)"
            >
              <div class="name">
                {{ p.name }}
                <span v-if="p.freeModel" class="badge free">{{ $t('免费') }}</span>
                <span v-else-if="p.recommended" class="badge info">{{ $t('推荐') }}</span>
              </div>
              <div class="desc">{{ p.note }}</div>
              <div class="tick">✓</div>
            </div>
          </div>
          <div class="small muted">
            {{ $t('智谱 GLM-4-Flash 免费可用；DeepSeek 性价比高。任意 OpenAI 兼容接口均可接入。') }}
          </div>
        </section>

        <section class="card stack">
          <div class="card-title" style="font-size: 15px">
            {{ currentPreset?.requiresKey === false ? $t('第二步：安装并运行本地模型') : $t('第二步：获取 API Key') }}
          </div>

          <!-- 本地模型：不用 Key，先确认服务在跑 -->
          <template v-if="currentPreset?.requiresKey === false">
            <div class="small muted">{{ $t('用本地模型不需要 API Key：装好并启动 Ollama 后直接测试即可。') }}</div>
            <div class="row wrap">
              <button class="btn" @click="openKeyPage">{{ $t('去下载 Ollama ↗') }}</button>
              <button class="btn btn-primary" :disabled="ollamaBusy" @click="probeOllama">
                {{ ollamaBusy ? $t('检测中…') : $t('检测本机 Ollama') }}
              </button>
            </div>
            <div
              v-if="ollama"
              class="hint-bar"
              :class="ollama.running ? 'info' : 'warn'"
            >
              {{
                ollama.running
                  ? $t('已检测到本机 Ollama，可选择模型：{a}', { a: ollama.models.join('、') || $t('尚未拉取模型') })
                  : $t('未检测到本机 Ollama：请先安装并启动（默认端口 11434）')
              }}
            </div>
          </template>

          <!-- 云端模型：去官方页面领 Key -->
          <template v-else>
            <div class="small muted">
              {{ $t('还没有 Key？点下面的按钮打开厂商官方页面，登录后创建并复制 API Key（只需一次）。') }}
            </div>
            <div class="row wrap">
              <button class="btn btn-primary" @click="openKeyPage">{{ $t('去申请 API Key ↗') }}</button>
              <span class="small muted mono" style="word-break: break-all">{{ currentPreset?.keyUrl }}</span>
            </div>
            <div class="hint-bar info">
              {{ $t('申请一般需要注册（国内平台多需实名）；复制时注意别漏字符、别带空格。') }}
            </div>
            <div class="small muted">
              {{ $t('页面打不开？登录厂商官网后，在控制台里找「API Keys / 密钥管理」即可。') }}
            </div>
          </template>
        </section>

        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('第三步：填入 Key 并测试连接') }}</div>
          <div class="form-row">
            <label class="form-label">
              API Key
              <span v-if="currentPreset?.requiresKey === false" class="muted small">{{ $t('（本地模型不需要）') }}</span>
              <span v-else class="muted small">{{ $t('（仅存于本机系统安全存储，界面不回读）') }}</span>
            </label>
            <div class="row">
              <input
                v-model="apiKeyInput"
                type="password"
                class="input"
                :placeholder="aiConfig.hasKey ? $t('已配置（留空则不修改）') : $t('粘贴你的 API Key')"
                style="flex: 1"
              />
              <span v-if="aiConfig.hasKey" class="badge ok">{{ $t('已配置') }}</span>
            </div>
          </div>
          <div class="form-row">
            <label class="form-label">
              {{ $t('模型') }}
              <span class="muted small">{{ $t('（切换模型不会改动 Base URL）') }}</span>
            </label>
            <div class="row">
              <div class="select-wrap" style="flex: 1">
                <select class="input" :value="aiConfig.model" @change="aiConfig.model = ($event.target as HTMLSelectElement).value">
                  <option v-for="m in modelOptions" :key="m" :value="m">{{ m }}</option>
                  <option v-if="!modelOptions.includes(aiConfig.model)" :value="aiConfig.model">
                    {{ aiConfig.model || $t('（自定义）') }}
                  </option>
                </select>
              </div>
              <input v-model="aiConfig.model" class="input mono" style="flex: 1" :placeholder="$t('或手动输入模型名')" />
            </div>
          </div>
          <div class="row wrap">
            <button class="btn btn-primary" :disabled="savingAi" @click="saveAi">{{ $t('保存配置') }}</button>
            <button class="btn" :disabled="testing" @click="testConnection">
              {{ testing ? $t('测试中…') : $t('保存并测试连接') }}
            </button>
            <span class="small muted">{{ $t('测试会先按当前配置保存一次，确保测的就是接下来要用的配置。') }}</span>
          </div>

          <!-- 测试结论：成功给出实测延迟与回复，失败给出「下一步改什么」 -->
          <div v-if="testResult" class="stack">
            <div v-if="testResult.ok" class="hint-bar info">
              <b style="color: var(--success)">
                ✓ {{ $t('连接成功 · 延迟 {a}ms · 模型 {b}', { a: testResult.latencyMs, b: testResult.model }) }}
              </b>
              <div v-if="testResult.reply" class="small muted">{{ $t('模型回复：{a}', { a: testResult.reply }) }}</div>
            </div>
            <div v-else class="hint-bar warn stack">
              <div>
                <b style="color: var(--danger)">✗ {{ $t('连接失败') }}</b>
                <span v-if="testResult.upstreamStatus" class="small muted"> · HTTP {{ testResult.upstreamStatus }}</span>
                <span v-if="testResult.model" class="small muted"> · {{ testResult.model }}</span>
              </div>
              <div>{{ failHint }}</div>
              <div v-if="testResult.detail" class="small muted mono" style="word-break: break-all">
                {{ $t('厂商原始返回：') }}{{ testResult.detail }}
              </div>
            </div>
          </div>

          <div class="hint-bar info">
            {{ $t('API Key 只保存于本机系统安全存储（Windows 凭据管理器），界面不回读；AI 请求由本机直连厂商，不经过智伴服务器。') }}
          </div>
        </section>

        <section class="card stack">
          <div class="row">
            <div class="card-title" style="font-size: 15px">{{ $t('高级设置（Base URL / 协议 / 采样参数）') }}</div>
            <div class="spacer"></div>
            <button class="btn btn-sm" @click="showAdvanced = !showAdvanced">
              {{ showAdvanced ? $t('收起 ▴') : $t('展开 ▾') }}
            </button>
          </div>
          <template v-if="showAdvanced">
            <div class="form-row">
              <label class="form-label">
                API Base URL
                <span class="muted small">{{ $t('（可自由填写，保存后不会被切换模型重置）') }}</span>
                <span v-if="baseUrlCustomized" class="badge info">{{ $t('自定义') }}</span>
              </label>
              <input v-model="aiConfig.baseUrl" class="input mono" :placeholder="$t('https://…/v1 或 …/anthropic')" />
              <div class="row wrap" style="gap: 6px; margin-top: 4px">
                <button class="btn btn-sm" @click="useDefaultUrl">{{ $t('恢复该提供商默认地址') }}</button>
                <button
                  v-if="currentPreset?.anthropicUrl"
                  class="btn btn-sm"
                  @click="useAnthropicUrl"
                >
                  {{ $t('使用 Anthropic 兼容地址') }}
                </button>
              </div>
            </div>
            <div class="form-row">
              <label class="form-label">
                {{ $t('接口协议') }}
                <span class="muted small">{{ $t('（默认自动识别：地址含 /anthropic 时使用 Anthropic 协议）') }}</span>
              </label>
              <div class="row">
                <div class="select-wrap" style="width: 240px">
                  <select v-model="aiConfig.protocolMode" class="input">
                    <option value="auto">{{ $t('自动识别（推荐）') }}</option>
                    <option value="openai">{{ $t('OpenAI 兼容（/chat/completions）') }}</option>
                    <option value="anthropic">{{ $t('Anthropic 兼容（/v1/messages）') }}</option>
                  </select>
                </div>
                <span class="badge" :class="aiConfig.detectedProtocol === 'anthropic' ? 'info' : 'ok'">
                  {{ $t('将使用 {a}', { a: aiConfig.detectedProtocol === 'anthropic' ? $t('Anthropic 协议 /v1/messages') : $t('OpenAI 协议 /chat/completions') }) }}
                </span>
              </div>
            </div>
            <div class="form-row">
              <label class="form-label">{{ $t('温度 {a}', { a: aiConfig.temperature }) }}</label>
              <input v-model.number="aiConfig.temperature" type="range" min="0" max="1.5" step="0.1" />
            </div>
            <div class="form-row">
              <label class="form-label">{{ $t('最大输出 tokens') }}</label>
              <input v-model.number="aiConfig.maxTokens" type="number" class="input" style="width: 160px" />
            </div>
          </template>
        </section>

        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('Prompt 模板（可自定义）') }}</div>
          <div class="seg wrap">
            <button
              v-for="t in [['daily', '日报'], ['weekly', '周报'], ['monthly', '月报'], ['brief', '我的简报'], ['goodnight', '晚安总结'], ['review', '复盘'], ['qa', '问答']]"
              :key="t[0]"
              :class="{ on: editingTemplate === t[0] }"
              @click="loadTemplate(t[0])"
            >
              {{ t[1] }}
            </button>
          </div>
          <textarea v-model="templateDraft" class="textarea" rows="10" style="font-family: ui-monospace, monospace; font-size: 12px"></textarea>
          <div class="row">
            <span class="small muted">{{ $t('可用变量：{a}', { a: templateVars }) }}</span>
            <div class="spacer"></div>
            <span class="small muted">{{ templateSaved }}</span>
            <button class="btn btn-sm" @click="resetTemplate">{{ $t('恢复默认') }}</button>
            <button class="btn btn-sm btn-primary" @click="saveTemplate">{{ $t('保存模板') }}</button>
          </div>
        </section>
      </template>

      <!-- 每日目标 -->
      <template v-if="section === 'goal'">
        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('每日记录目标') }}</div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">{{ $t('启用每日目标（驱动进度条）') }}</span>
            <div class="switch" :class="{ on: goalEnabled }" @click="goalEnabled = !goalEnabled"></div>
          </div>
          <div class="form-row">
            <label class="form-label">{{ $t('每天希望记录多少条（默认 4）') }}</label>
            <div class="row">
              <input v-model.number="dailyGoal" type="number" min="1" max="50" class="input" style="width: 120px" />
              <span class="small muted">{{ $t('条 / 天') }}</span>
            </div>
          </div>
          <div class="hint-bar info">{{ $t('记录一条即一个节点，达标后继续记录会显示「超额」') }}</div>
          <div class="row">
            <div class="spacer"></div>
            <button class="btn btn-primary" @click="saveGoal">{{ $t('保存') }}</button>
          </div>
        </section>
      </template>

      <!-- 外观 -->
      <template v-if="section === 'appearance'">
        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('主题') }}</div>
          <div class="preset-grid">
            <div
              v-for="t in THEMES"
              :key="t.key"
              class="preset-card"
              :class="{ on: themeMode === t.key }"
              @click="app.applyTheme(t.key)"
            >
              <div class="name">{{ t.label }}</div>
              <div class="desc">{{ t.desc }}</div>
              <div class="tick">✓</div>
            </div>
          </div>
          <div class="small muted">
            {{ $t('当前解析主题：{a}（跟随系统模式下会随系统设置实时切换）', { a: app.resolvedTheme === 'dark' ? $t('深色') : $t('浅色') }) }}
          </div>
        </section>

        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('界面语言') }}</div>
          <div class="preset-grid">
            <div
              v-for="m in localeOptions"
              :key="m"
              class="preset-card"
              :class="{ on: localeMode === m }"
              @click="changeLocale(m)"
            >
              <div class="name">{{ LOCALE_LABELS[m] }}</div>
              <div class="desc">{{ m === 'system' ? $t('当前：{a}', { a: LOCALE_LABELS[effectiveLocale] }) : m }}</div>
              <div class="tick">✓</div>
            </div>
          </div>
          <div class="small muted">
            {{ $t('默认跟随系统语言；AI 报告、我的简报与提醒文案也会使用该语言生成。') }}
          </div>
        </section>
      </template>

      <!-- 数据与部署 -->
      <template v-if="section === 'data'">
        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('数据备份与恢复') }}</div>
          <div class="row wrap">
            <button class="btn btn-primary" @click="doExport">{{ $t('⬇️ 导出全部数据（JSON）') }}</button>
            <span class="small muted">{{ $t('包含记录、待办、设置与报告') }}</span>
          </div>
          <div class="row wrap">
            <button class="btn" :disabled="exportingMd" @click="doExportMarkdown">
              {{ exportingMd ? $t('打包中…') : $t('📦 导出 Markdown 归档（ZIP）') }}
            </button>
            <span class="small muted">
              {{ $t('按周期分文件：daily/ 每日 · weekly/ 每周 · monthly/ 每月 · reports/ 报告') }}
            </span>
          </div>
          <div class="form-row">
            <label class="form-label">{{ $t('导入数据（粘贴备份 JSON）') }}</label>
            <textarea v-model="importText" class="textarea" rows="4" placeholder='{"version":1,"nodes":[...]}'></textarea>
          </div>
          <div class="row">
            <label class="row small" style="gap: 6px">
              <input v-model="wipeOnImport" type="checkbox" />
              {{ $t('导入前清空现有数据') }}
            </label>
            <div class="spacer"></div>
            <button class="btn" @click="doImport">{{ $t('导入') }}</button>
          </div>
        </section>

        <section class="card stack">
          <div class="card-title" style="font-size: 15px">{{ $t('部署模式与访问') }}</div>
          <div class="small muted">
            {{ $t('当前模式：') }}<b>{{ app.auth?.mode === 'local' ? $t('本地模式（仅本机）') : app.auth?.mode === 'lan' ? $t('局域网模式') : $t('服务器模式') }}</b>
            {{ $t('—— 修改需重启应用并调整启动参数（--mode lan / --mode server --headless）') }}
          </div>
          <div class="form-row">
            <label class="form-label">{{ $t('访问密码（局域网/服务器模式登录用）') }}</label>
            <div class="row">
              <input v-model="password" type="password" class="input" style="flex: 1; max-width: 260px" :placeholder="$t('至少 6 位')" />
              <button class="btn" :disabled="savingPassword" @click="savePassword">
                {{ app.auth?.hasPassword ? $t('修改密码') : $t('设置密码') }}
              </button>
            </div>
          </div>
          <div class="hint-bar warn">
            {{ $t('⚠️ 浏览器访问本机服务：http://127.0.0.1:17801 ；手机/平板在同一局域网且开启局域网模式时可访问') }}
          </div>
        </section>
      </template>

      <!-- 关于 -->
      <template v-if="section === 'about'">
        <section class="card stack">
          <div class="row">
            <div class="logo" style="width: 46px; height: 46px; font-size: 20px">M</div>
            <div>
              <div class="card-title" style="font-size: 16px">{{ $t('智伴 Mindmate') }}</div>
              <div class="small muted">{{ $t('v{a} · AI 工作生活伴侣', { a: update.currentVersion || '1.0.0' }) }}</div>
            </div>
          </div>
          <div class="small" style="color: var(--text-regular); line-height: 1.8">
            <div>· <b>{{ $t('数据本地') }}</b>{{ $t('：全部内容存于本机 SQLite，除你主动生成 AI 内容外不联网') }}</div>
            <div>· <b>{{ $t('双端一致') }}</b>{{ $t('：桌面端与浏览器端共享同一份数据与 AI 能力') }}</div>
            <div>· <b>{{ $t('AI 可换') }}</b>{{ $t('：支持 DeepSeek / 智谱 GLM / OpenAI / 通义 / Kimi / 本地 Ollama') }}</div>
            <div>· <b>{{ $t('提醒克制') }}</b>{{ $t('：达标静默、逾期加压、连续未记才温柔提醒') }}</div>
          </div>
          <div class="row">
            <span class="small muted">{{ $t('服务状态：') }}</span>
            <span class="badge" :class="app.connected ? 'ok' : 'danger'">{{ app.connected ? $t('运行中') : $t('未连接') }}</span>
          </div>

          <!-- 版本与更新 -->
          <div class="divider"></div>
          <div class="row">
            <span style="flex: 1; font-size: 13px">{{ $t('自动检查更新') }}</span>
            <div
              class="switch"
              :class="{ on: update.autoCheckEnabled }"
              :style="!isDesktop() ? 'opacity:.5;cursor:not-allowed' : ''"
              @click="isDesktop() && app.saveSettings({ auto_update_check: update.autoCheckEnabled ? '0' : '1' })"
            ></div>
          </div>
          <div class="hint-bar" v-if="!isDesktop()">
            {{ $t('浏览器端不支持自动更新，请从下载页获取新版本。') }}
          </div>
          <div class="row">
            <span class="small muted" style="flex: 1">
              {{ update.lastCheckedAt ? $t('上次检查：{a}', { a: update.lastCheckedAt }) : $t('尚未检查过新版本') }}
              <span v-if="update.skippedVersion"> {{ $t('· 已跳过 v{a}', { a: update.skippedVersion }) }}</span>
            </span>
            <button
              class="btn btn-sm"
              :disabled="!isDesktop() || update.checking || update.installing"
              @click="update.check(true)"
            >
              {{ update.checking ? $t('检查中…') : $t('检查更新') }}
            </button>
          </div>
          <div v-if="update.available" class="row" style="gap: 8px">
            <span class="small" style="color: var(--primary)">{{ $t('发现新版本 v{a}', { a: update.available.version }) }}</span>
            <button class="btn btn-sm btn-primary" :disabled="update.installing" @click="update.install()">
              {{ update.installing ? $t('更新中 {a}%', { a: update.progress || 0 }) : $t('下载并重启') }}
            </button>
          </div>
          <div v-else-if="update.upToDate" class="small muted">{{ $t('已是最新版本') }}</div>
          <div v-if="update.error" class="small" style="color: var(--danger)">{{ update.error }}</div>
          <div class="small muted" style="line-height: 1.7">
            {{ $t('更新包由官方私钥签名，客户端验签通过才会安装；更新不会触碰你的数据目录，安装失败也不会影响现有版本。') }}
          </div>
        </section>
      </template>
    </div>
  </div>
</template>
