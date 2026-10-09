<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useAppStore, todayStr, weekdayLabel, friendlyDate, addDays } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { useTodosStore } from '@/stores/todos'
import { api, streamApi } from '@/api/client'
import type { AppEvent } from '@/api/types'
import QuickEntry from '@/components/QuickEntry.vue'
import NodeItem from '@/components/NodeItem.vue'
import TodoItem from '@/components/TodoItem.vue'
import ProgressPair from '@/components/ProgressPair.vue'
import MarkdownView from '@/components/MarkdownView.vue'
import TodoEditModal from '@/components/TodoEditModal.vue'
import TodoDetailModal from '@/components/TodoDetailModal.vue'
import SmartTodoModal from '@/components/SmartTodoModal.vue'
import AchievementsDrawer from '@/components/AchievementsDrawer.vue'
import type { AchievementDef, NewsItem, Todo, ExtractedTodo } from '@/api/types'
import { isDesktop } from '@/lib/desktop'
import { t } from '@/i18n'

const app = useAppStore()
const nodes = useNodesStore()
const todos = useTodosStore()

const briefOpen = ref(true)
const briefContent = ref('')
const briefLoading = ref(false)
const briefIsAi = ref(true)
const briefAt = ref('')
let abortBrief: (() => void) | null = null
const showTodoModal = ref(false)
const presetTodo = ref<{ title?: string; description?: string; tags?: string[] } | null>(null)
const showAchievements = ref(false)
const badges = ref<AchievementDef[]>([])

// ── 今日热点（我的简报旁的 tag，点击切换面板）──
// 面板选择持久化：切走再回来 / 重启应用都保留上次的选择
const PANEL_KEY = 'mindmate_today_panel'
const panel = ref<'brief' | 'news'>(
  localStorage.getItem(PANEL_KEY) === 'news' ? 'news' : 'brief'
)
const newsItems = ref<NewsItem[]>([])
const newsLoading = ref(false)
const newsSource = ref('')
const newsStale = ref(false)
const newsErrors = ref<string[]>([])
// 下滑加载更多：每次向下滚动多要一页（页大小=设置条数），后端 30 分钟缓存使翻页请求很便宜
const PAGE_SIZE_FALLBACK = 10
const newsDisplayLimit = ref(PAGE_SIZE_FALLBACK)
const newsLoadingMore = ref(false)
const newsNoMore = ref(false)
const newsUpdatedAt = ref('')
// 自动更新：设置里开启后，热点面板打开期间按频率自动重新抓取
let newsAutoTimer: ReturnType<typeof setInterval> | null = null

/** 重点关注配置（设置里选的行业 + 自定义关键词），空 = 不过滤 */
function newsFocusParams(): { topics: string[]; keywords: string[] } {
  const topics = (() => {
    try {
      const f = JSON.parse(app.settings.news_focus || '[]')
      return Array.isArray(f) ? f : []
    } catch {
      return []
    }
  })()
  const keywords = (() => {
    try {
      const k = JSON.parse(app.settings.news_focus_keywords || '[]')
      return Array.isArray(k) ? k : []
    } catch {
      return []
    }
  })()
  return { topics, keywords }
}

function newsPageSize(): number {
  const n = Number(app.settings.news_limit || PAGE_SIZE_FALLBACK)
  return Math.min(Math.max(n, 5), 50)
}

function dedupeNews(items: NewsItem[]): NewsItem[] {
  const seen = new Set<string>()
  return items.filter((n) => {
    const key = `${n.channel}|${n.title}|${n.url}`
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

async function loadNews(refresh = false, limit?: number) {
  if (newsLoading.value || newsLoadingMore.value) return
  if (refresh) newsLoading.value = true
  else newsLoadingMore.value = true
  const want = Math.min(limit ?? newsDisplayLimit.value, 200)
  const focus = newsFocusParams()
  const filtering = focus.topics.length > 0 || focus.keywords.length > 0
  try {
    const r = await api.hotNews(refresh, want, filtering ? focus : undefined)
    const merged = dedupeNews([...(r.items || [])])
    newsItems.value = merged
    newsSource.value = r.source
    newsStale.value = r.stale === true
    newsErrors.value = r.errors || []
    newsUpdatedAt.value = (r.fetchedAt || '').slice(11, 16)
    newsNoMore.value = !filtering && merged.length < want
    if (refresh) app.toast('success', t('已更新 {a} 条热点', { a: merged.length }))
  } catch (e: any) {
    app.toast('error', e?.message || t('热点获取失败，请稍后再试'))
  } finally {
    newsLoading.value = false
    newsLoadingMore.value = false
  }
}

/** 下滑到底部 → 多取一页 */
function onNewsScroll(e: Event) {
  const el = e.target as HTMLElement
  if (!el) return
  const nearBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 24
  if (!nearBottom || newsLoadingMore.value || newsLoading.value || newsNoMore.value) return
  const pageSize = newsPageSize()
  loadNews(false, newsDisplayLimit.value + pageSize).then(() => {
    newsDisplayLimit.value = Math.min(newsDisplayLimit.value + pageSize, 200)
  })
}

function isFocusFiltering(): boolean {
  const f = newsFocusParams()
  return f.topics.length > 0 || f.keywords.length > 0
}

function focusSummary(): string {
  const f = newsFocusParams()
  const names = f.topics
    .map((id) => FOCUS_NAME[id] || id)
    .concat(f.keywords.length ? [t('自定义 {a} 个', { a: f.keywords.length })] : [])
  return names.join('、')
}

const FOCUS_NAME: Record<string, string> = {
  ai: 'AI', edu: t('教育'), agri: t('农业'), med: t('医疗'),
  fin: t('财经'), auto: t('汽车'), tech: t('科技'), sport: t('体育'),
}

/** 自动更新：按设置频率定时重抓（仅热点面板打开期间挂定时器） */
function syncNewsAutoTimer() {
  const enabled = app.settings.news_auto_refresh === '1'
  const minutes = Math.max(Number(app.settings.news_refresh_minutes || 30), 1)
  if (newsAutoTimer) { clearInterval(newsAutoTimer); newsAutoTimer = null }
  if (enabled && panel.value === 'news') {
    newsAutoTimer = setInterval(() => loadNews(true), minutes * 60 * 1000)
  }
}

function settingsChanged() {
  // 条数设置变化后，下次展开按新页大小加载
  if (!newsItems.value.length) newsDisplayLimit.value = newsPageSize()
  syncNewsAutoTimer()
}

/** 点击 tag 切换简报/热点面板；选择持久化 + 切到热点即自动刷新（强制实抓） */
function toggleNewsPanel() {
  panel.value = panel.value === 'news' ? 'brief' : 'news'
  localStorage.setItem(PANEL_KEY, panel.value)
  if (panel.value === 'news') {
    loadNews(true)
    syncNewsAutoTimer()
  } else if (newsAutoTimer) {
    clearInterval(newsAutoTimer)
    newsAutoTimer = null
  }
}

/** 点击新闻 → 系统浏览器打开（桌面端走本机内核，浏览器端开新标签） */
async function openNews(n: NewsItem) {
  if (!n.url) {
    app.toast('warning', t('该条新闻没有可打开的链接'))
    return
  }
  if (isDesktop()) {
    try {
      await api.openUrl(n.url)
    } catch {
      app.toast('warning', t('打开链接失败，请手动复制到浏览器：{a}', { a: n.url }))
    }
    return
  }
  window.open(n.url, '_blank', 'noopener')
}

// ── 热点一键转记录 / 转待办（v1.1.1） ──
const convertingNews = ref<string | null>(null) // 正在转的条目标题（按钮防抖）

const unlockedBadges = computed(() => badges.value.filter((a) => a.unlocked))

// ── 新手引导任务流（v1.2.0-③）：从现有数据推导完成态，全部完成自动收起并持久化 ──
const guideSteps = computed(() => [
  { key: 'node', label: t('完成第一条速记'), done: (app.stats?.nodeCount ?? 0) > 0 || nodes.nodes.length > 0 },
  { key: 'todo', label: t('创建第一个待办'), done: todos.todos.length > 0 },
  { key: 'brief', label: t('生成第一份简报'), done: !!briefContent.value },
  { key: 'ai', label: t('配置 AI 模型（可选，解锁智能打标/周计划）'), done: app.aiReady, optional: true }
])
const guideAllDone = computed(() => guideSteps.value.filter((x) => !x.optional).every((x) => x.done))
const showGuide = ref(app.settings.guide_done !== '1')

function dismissGuide() {
  showGuide.value = false
  app.saveSettings({ guide_done: '1' }).catch(() => {
    localStorage.setItem('mindmate_guide_done', '1')
  })
}

watch(
  guideAllDone,
  (v) => {
    if (v && showGuide.value) dismissGuide()
  },
  { immediate: true }
)

async function loadBadges() {
  try {
    badges.value = await api.checkAchievements()
  } catch {
    /* 忽略：成就非核心路径 */
  }
}

const today = computed(() => todayStr())
const dateLabel = computed(() => `${today.value} · ${weekdayLabel(today.value)}`)

// ── 记录日期切换（用户需求）：今日记录列表可切换查看其他日期 ──
const viewDate = ref(todayStr())
const isToday = computed(() => viewDate.value === today.value)
const viewDateLabel = computed(() =>
  isToday.value ? t('今日记录') : `${friendlyDate(viewDate.value)} · ${t('记录')}`
)

async function shiftViewDate(days: number) {
  viewDate.value = addDays(viewDate.value, days)
  await nodes.load(viewDate.value)
}

async function setViewDate(e: Event) {
  const v = (e.target as HTMLInputElement).value
  if (!v) return
  viewDate.value = v
  await nodes.load(v)
}

async function backToToday() {
  viewDate.value = today.value
  await nodes.load(today.value)
}

const todayTodoList = computed(() =>
  todos.todos
    .filter((t) => {
      if (t.dueDate <= today.value || t.status === '已逾期') return true
      // 有效期窗口覆盖今天也算今日可做（v1.5.3：startDate ≤ 今天 ≤ dueDate，用户反馈）
      return !!t.startDate && t.startDate <= today.value && t.dueDate >= today.value
    })
    .sort((a, b) => (a.status === '已完成' ? 1 : 0) - (b.status === '已完成' ? 1 : 0))
)
const todoDone = computed(() => todayTodoList.value.filter((t) => t.status === '已完成').length)

const recordLabel = computed(() => {
  const s = app.stats
  if (!s || !s.goalEnabled) return t('{a} 条', { a: nodes.nodes.length })
  const over = s.nodeCount > s.dailyGoal ? t(' · 超额 {a}', { a: s.nodeCount - s.dailyGoal }) : ''
  return t('{a}/{b} 条', { a: s.nodeCount, b: s.dailyGoal }) + over
})

async function loadBrief() {
  // 读取已有简报（当天）
  try {
    const list = await api.reports('brief')
    const found = list.find((r) => r.period === today.value)
    if (found) {
      briefContent.value = found.content
      briefIsAi.value = found.isAi
      briefAt.value = found.createdAt.slice(11, 16)
      return
    }
  } catch {
    /* 忽略 */
  }
  briefContent.value = ''
}

function generateBrief() {
  if (briefLoading.value) return
  briefLoading.value = true
  briefContent.value = ''
  briefOpen.value = true
  abortBrief = streamApi.brief(today.value, {
    onDelta: (t, degraded) => {
      briefContent.value += t
      if (degraded) briefIsAi.value = false
    },
    onDone: () => {
      briefLoading.value = false
      briefAt.value = new Date().toTimeString().slice(0, 5)
    },
    onError: (msg) => {
      briefLoading.value = false
      app.toast('error', msg)
    }
  })
}

function stopBrief() {
  abortBrief?.()
  briefLoading.value = false
}

function onGlobalEvent(e: Event) {
  const detail = (e as CustomEvent).detail as AppEvent
  if (detail.kind === 'report.saved' && detail.payload?.type === 'brief') loadBrief()
}

watch(
  () => [app.settings.news_limit, app.settings.news_auto_refresh, app.settings.news_refresh_minutes],
  () => settingsChanged()
)

onMounted(async () => {
  window.addEventListener('mindmate:event', onGlobalEvent)
  // 恢复上次的面板选择：只补数据与定时器，**不强制刷新**（用户反馈：从其他页面
  // 切过来不要重新抓取，直接展示缓存内容）；只有手动从简报切到热点才强刷。
  if (panel.value === 'news') {
    loadNews(false)
    syncNewsAutoTimer()
  }
  nodes.date = today.value
  await Promise.all([
    nodes.load(viewDate.value),
    todos.load(),
    loadBrief(),
    app.refreshStats(),
    app.refreshAiReady(),
    loadBadges()
  ])
  // 到点自动生成简报（若开启且尚无）
  if (app.settings.smart_brief_enabled === '1' && !briefContent.value) {
    const now = new Date()
    const target = Number(app.settings.brief_minutes || 540)
    if (now.getHours() * 60 + now.getMinutes() >= target) generateBrief()
  }
})
onUnmounted(() => {
  window.removeEventListener('mindmate:event', onGlobalEvent)
  if (newsAutoTimer) { clearInterval(newsAutoTimer); newsAutoTimer = null }
  abortBrief?.()
})

// ── 提炼行动项（v1.4.1）：勾选今日记录 → AI/规则提炼待办 ──
const selecting = ref(false)
const selectedNodes = ref<number[]>([])
const showActionModal = ref(false)
const actionItems = ref<ExtractedTodo[]>([])
const actionIsAi = ref(false)
const actionLoading = ref(false)

function toggleSelect(id: number) {
  const i = selectedNodes.value.indexOf(id)
  if (i >= 0) selectedNodes.value.splice(i, 1)
  else selectedNodes.value.push(id)
}

function toggleSelecting() {
  selecting.value = !selecting.value
  selectedNodes.value = []
}

async function extractActions() {
  if (actionLoading.value || !selectedNodes.value.length) return
  actionLoading.value = true
  try {
    const contents = nodes.nodes
      .filter((n) => selectedNodes.value.includes(n.id))
      .map((n) => n.content)
    const r = await api.aiActionItems(contents)
    actionItems.value = r.items
    actionIsAi.value = r.isAi
    if (!r.items.length) {
      app.toast('info', t('没有提炼出行动项'))
      return
    }
    showActionModal.value = true
  } catch (e: any) {
    app.toast('error', e?.message || t('提炼失败，请稍后再试'))
  } finally {
    actionLoading.value = false
  }
}

// ── 待办详情（v1.2.2）：点行看详情 ──
const detailTodo = ref<Todo | null>(null)
const editingTodo = ref<Todo | null>(null)
const showEditModal = ref(false)

function openDetail(t: Todo) {
  detailTodo.value = t
}

async function onDetailToggle(id: number) {
  await todos.toggle(id)
  detailTodo.value = null
  await todos.load()
  app.refreshStats()
}

async function onDetailPin(todo: Todo) {
  const pinned = todo.sortOrder < 0
  await todos.update(todo.id, { sortOrder: pinned ? 0 : -1 })
  detailTodo.value = null
  await todos.load()
  app.toast('success', pinned ? t('已取消置顶') : t('已置顶，将显示在最前'))
}

// 一键把今日待办完成
async function completeTodo(id: number) {
  await todos.toggle(id)
  await app.refreshStats()
}
</script>

<template>
  <div class="grid-today">
    <div class="col-stack">
      <!-- 新手引导（v1.2.0）：四步走完核心路径 -->
      <section v-if="showGuide && !guideAllDone" class="card guide-card">
        <div class="row" style="margin-bottom: 6px">
          <div class="card-title" style="font-size: 15px">{{ $t('👋 三步上手智伴') }}</div>
          <div class="spacer"></div>
          <button class="btn btn-sm" @click="dismissGuide">{{ $t('跳过引导') }}</button>
        </div>
        <div class="row wrap" style="gap: 8px">
          <span
            v-for="st in guideSteps"
            :key="st.key"
            class="badge"
            :class="st.done ? 'ok' : 'info'"
            :style="st.done ? '' : 'opacity:.75'"
          >
            {{ st.done ? '✓' : '○' }} {{ st.label }}
          </span>
        </div>
      </section>

      <!-- 我的简报 + 今日热点 -->
      <section class="card">
        <div class="row" style="align-items: flex-start">
          <div
            style="width: 40px; height: 40px; border-radius: 50%; background: var(--primary-weak); color: var(--primary); display: grid; place-items: center; font-size: 18px; flex: none"
          >
            📋
          </div>
          <div style="flex: 1; min-width: 0">
            <div class="card-title" style="font-size: 15px; display: flex; align-items: center; gap: 8px">
              <span>{{ panel === 'brief' ? $t('我的简报') : $t('今日热点') }}</span>
              <button
                class="news-tag"
                :class="{ on: panel === 'news' }"
                :title="$t('切换到今日热点 / 我的简报')"
                @click="toggleNewsPanel"
              >
                {{ panel === 'news' ? $t('我的简报') : $t('今日热点') }}
              </button>
              <span v-if="panel === 'brief'" class="card-sub">{{ briefAt ? $t('{a} 生成', { a: briefAt }) : $t('尚未生成') }}</span>
            </div>
            <div v-if="panel === 'brief' && !briefContent && !briefLoading" class="small muted" style="margin-top: 4px">
              {{ $t('看看今天的安排，让智伴帮你理一理。') }}
            </div>
          </div>
          <template v-if="panel === 'brief'">
            <button v-if="briefLoading" class="btn btn-sm" @click="stopBrief">{{ $t('停止') }}</button>
            <button v-else class="btn btn-sm btn-primary" @click="generateBrief">
              {{ briefContent ? $t('重新生成') : $t('生成简报') }}
            </button>
            <button v-if="briefContent" class="icon-btn" @click="briefOpen = !briefOpen">
              {{ briefOpen ? '▾' : '▸' }}
            </button>
          </template>
          <button v-else class="btn btn-sm btn-primary" :disabled="newsLoading" @click="loadNews(true)">
            {{ newsLoading ? $t('抓取中…') : $t('重新生成') }}
          </button>
        </div>
        <!-- 今日热点面板：点击条目用系统浏览器打开 -->
        <div v-if="panel === 'news'" style="margin-top: 12px">
          <div v-if="!newsItems.length && newsLoading" class="small muted" style="padding: 12px 0">{{ $t('正在抓取互联网热点…') }}</div>
          <div v-else-if="!newsItems.length && isFocusFiltering()" class="empty" style="padding: 18px 0">
            <div class="ill">🎯</div>
            <div class="t">{{ $t('当前重点关注下暂时没有匹配的热点') }}</div>
            <div class="d">{{ $t('可到设置调整行业与自定义关键词') }}</div>
          </div>
          <div v-else-if="!newsItems.length" class="empty" style="padding: 18px 0">
            <div class="ill">📡</div>
            <div class="t">{{ $t('暂时拉不到热点') }}</div>
            <div class="d">{{ $t('检查网络后点「重新生成」再试试') }}</div>
          </div>
          <template v-else>
            <div v-if="newsStale" class="hint-bar warn" style="margin-bottom: 8px">
              {{ $t('本次抓取失败，正在展示最近一次成功的数据') }}
            </div>
            <!-- 下滑到底部自动加载更多；条目悬停出现「转记录 / 转待办」 -->
            <ol class="news-list" @scroll.passive="onNewsScroll">
              <li v-for="(n, i) in newsItems" :key="`${n.channel}-${i}`">
                <span class="news-idx mono">{{ i + 1 }}</span>
                <a class="news-title" :title="n.title" @click="openNews(n)">{{ n.title }}</a>
                <span class="chip" style="flex: none">{{ n.channelName }}</span>
              </li>
              <li v-if="newsLoadingMore" class="small muted" style="justify-content: center">
                {{ $t('加载更多…') }}
              </li>
              <li v-else-if="newsNoMore" class="small muted" style="justify-content: center">
                {{ $t('没有更多了') }}
              </li>
            </ol>
            <div class="small muted" style="margin-top: 8px">
              <template v-if="isFocusFiltering()">
                {{ $t('已按重点关注过滤：{a}', { a: focusSummary() }) }}
              </template>
              <template v-else>
                {{ $t('栏目与条数可在设置中调整 · 点击条目用浏览器打开') }}
              </template>
              <span v-if="newsUpdatedAt"> · {{ $t('更新于 {a}', { a: newsUpdatedAt }) }}</span>
            </div>
          </template>
        </div>

        <div v-if="panel === 'brief' && briefOpen && (briefContent || briefLoading)" style="margin-top: 12px">
          <!-- AI 未配置：引导配置；已配置但这份简报是旧模板生成的：引导重新生成 -->
          <div v-if="!briefIsAi && briefContent && !app.aiReady" class="hint-bar warn" style="margin-bottom: 10px">
            {{ $t('⚙️ 当前为本地模板拼装，配置 AI 模型可获得更优质的简报') }}
            <router-link to="/settings" class="link">{{ $t('去配置') }}</router-link>
          </div>
          <div v-else-if="!briefIsAi && briefContent && app.aiReady" class="hint-bar info" style="margin-bottom: 10px">
            {{ $t('✨ AI 模型已配置 —— 这份简报是此前用本地模板生成的') }}
            <span class="link" @click="generateBrief">{{ $t('重新生成') }}</span>
          </div>
          <MarkdownView :content="briefContent" />
          <span v-if="briefLoading" class="stream-cursor"></span>
        </div>
      </section>

      <!-- 今日进度 -->
      <section class="card">
        <div class="row" style="margin-bottom: 10px">
          <div class="card-title" style="font-size: 15px">{{ $t('今日') }}</div>
          <div class="spacer"></div>
          <span v-if="app.stats && app.stats.streakDays > 0" class="streak">{{ $t('🔥 连续记录 {a} 天', { a: app.stats.streakDays }) }}</span>
        </div>
        <ProgressPair
          :node-count="app.stats?.nodeCount ?? nodes.nodes.length"
          :daily-goal="app.stats?.dailyGoal ?? 4"
          :goal-enabled="app.stats?.goalEnabled ?? true"
          :goal-label="recordLabel"
          :todo-done="todoDone"
          :todo-total="todayTodoList.length"
        />
      </section>

      <!-- 速记 -->
      <QuickEntry :date="viewDate" autofocus />

      <!-- 时间线 -->
      <section class="card">
        <div class="row" style="margin-bottom: 6px; flex-wrap: wrap; row-gap: 6px">
          <div class="card-title" style="font-size: 15px; white-space: nowrap">{{ viewDateLabel }}</div>
          <span class="card-sub" style="white-space: nowrap">{{ $t('{a} 条', { a: nodes.nodes.length }) }}</span>
          <div class="row" style="gap: 4px; align-items: center; flex: none">
            <button class="icon-btn" :title="$t('前一天')" @click="shiftViewDate(-1)">◀</button>
            <input type="date" class="input" style="width: 140px" :value="viewDate" @change="setViewDate" />
            <button class="icon-btn" :title="$t('后一天')" @click="shiftViewDate(1)">▶</button>
            <button v-if="!isToday" class="btn btn-sm" style="white-space: nowrap" @click="backToToday">{{ $t('回到今天') }}</button>
          </div>
          <div class="spacer"></div>
          <span v-if="!selecting" class="small muted rec-hint">{{ $t('按时刻排列 · 一次录入即一个节点') }}</span>
          <button
            v-if="nodes.nodes.length"
            class="btn btn-sm"
            :class="{ 'btn-primary': selecting }"
            @click="toggleSelecting"
          >
            {{ selecting ? $t('取消选择') : $t('✨ 提炼行动项') }}
          </button>
        </div>
        <div v-if="selecting" class="hint-bar info" style="margin-bottom: 8px">
          {{ $t('勾选会议记录、沟通摘录等 → 点下方「提炼」自动抽出待办（确认后入库并打「行动项」标签）') }}
          <button
            class="btn btn-sm btn-primary"
            style="margin-left: 8px"
            :disabled="!selectedNodes.length || actionLoading"
            @click="extractActions"
          >
            {{ actionLoading ? $t('提炼中…') : $t('提炼（{a}）', { a: selectedNodes.length }) }}
          </button>
        </div>
        <div v-if="nodes.nodes.length === 0" class="empty">
          <div class="ill">📝</div>
          <div class="t">{{ isToday ? $t('今天还没有记录') : $t('该日暂无记录') }}</div>
          <div class="d">{{ $t('记下第一笔，10 秒就好') }}</div>
        </div>
        <div v-else class="tl">
          <div
            v-for="(n, i) in nodes.nodes"
            :key="n.id"
            class="row"
            style="align-items: flex-start; gap: 4px"
          >
            <input
              v-if="selecting"
              type="checkbox"
              style="margin-top: 6px"
              :checked="selectedNodes.includes(n.id)"
              @change="toggleSelect(n.id)"
            />
            <div style="flex: 1; min-width: 0">
              <NodeItem :node="n" :show-line="i < nodes.nodes.length - 1" />
            </div>
          </div>
        </div>
      </section>
    </div>

    <!-- 右侧：今日待办 -->
    <div class="col-stack">
      <section class="card">
        <div class="row" style="margin-bottom: 10px">
          <div class="card-title" style="font-size: 15px">{{ $t('今日待办') }}</div>
          <div class="spacer"></div>
          <span class="small muted">{{ todoDone }}/{{ todayTodoList.length }}</span>
        </div>
        <div class="progress" :class="todoDone === todayTodoList.length && todayTodoList.length ? 'p-high' : 'p-mid'" style="margin-bottom: 10px">
          <i :style="{ width: (todayTodoList.length ? Math.round((todoDone / todayTodoList.length) * 100) : 0) + '%' }"></i>
        </div>
        <div v-if="todayTodoList.length === 0" class="empty" style="padding: 24px 0">
          <div class="ill">✨</div>
          <div class="t">{{ $t('此刻一身轻') }}</div>
          <div class="d">{{ $t('没有待办，或添加一件') }}</div>
        </div>
        <TodoItem v-for="t in todayTodoList" :key="t.id" :todo="t" @detail="openDetail" />
        <button class="btn" style="width: 100%; justify-content: center; margin-top: 8px" @click="showTodoModal = true">
          {{ $t('＋ 添加今日待办') }}
        </button>
      </section>

      <!-- 逾期提醒 -->
      <section v-if="todos.overdue.length" class="card">
        <div class="card-title" style="font-size: 15px; color: var(--danger)">{{ $t('⚠️ 已逾期 {a}', { a: todos.overdue.length }) }}</div>
        <div style="margin-top: 8px">
          <TodoItem v-for="t in todos.overdue.slice(0, 5)" :key="t.id" :todo="t" @detail="openDetail" />
        </div>
      </section>

      <!-- 成就（FR-6.2）：展示友好名称，点击查看全部徽章 -->
      <section class="card">
        <div class="row" style="margin-bottom: 8px">
          <div class="card-title" style="font-size: 15px">{{ $t('我的徽章') }}</div>
          <div class="spacer"></div>
          <button class="btn btn-sm" @click="showAchievements = true">{{ $t('全部徽章') }}</button>
        </div>
        <div v-if="unlockedBadges.length" class="row wrap" style="gap: 6px">
          <span v-for="a in unlockedBadges" :key="a.id" class="badge ok" :title="a.description">
            {{ a.icon }} {{ a.name }}
          </span>
        </div>
        <div v-else class="small muted">
          {{ $t('完成首次速记即可解锁第一枚徽章 ✍️ ——') }}
          <span class="link" @click="showAchievements = true">{{ $t('看看全部徽章') }}</span>
        </div>
      </section>
    </div>

    <AchievementsDrawer v-if="showAchievements" @close="showAchievements = false; loadBadges()" />

    <TodoEditModal
      v-if="showTodoModal"
      :default-date="today"
      :preset="presetTodo"
      @close="showTodoModal = false; presetTodo = null"
      @saved="showTodoModal = false; presetTodo = null; todos.load(); app.refreshStats()"
    />
  </div>
    <TodoDetailModal
      v-if="detailTodo"
      :todo="detailTodo"
      @close="detailTodo = null"
      @edit="(t) => { detailTodo = null; editingTodo = t; showEditModal = true }"
      @pin="onDetailPin"
      @toggle="onDetailToggle"
    />

    <!-- 编辑既有待办（v1.2.2：详情页跳转编辑） -->
    <SmartTodoModal
      v-if="showActionModal"
      :items="actionItems"
      :is-ai="actionIsAi"
      :source="t('今日记录提炼')"
      tag="行动项"
      @close="showActionModal = false"
      @saved="showActionModal = false; selecting = false; selectedNodes = []; todos.load(); app.refreshStats()"
    />

    <TodoEditModal
      v-if="showEditModal && editingTodo"
      :todo="editingTodo"
      @close="showEditModal = false; editingTodo = null"
      @saved="showEditModal = false; editingTodo = null; todos.load(); app.refreshStats()"
    />
</template>

<style scoped>
.rec-hint {
  white-space: nowrap;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
