<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
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
import AchievementsDrawer from '@/components/AchievementsDrawer.vue'
import type { AchievementDef } from '@/api/types'
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
const showAchievements = ref(false)
const badges = ref<AchievementDef[]>([])

const unlockedBadges = computed(() => badges.value.filter((a) => a.unlocked))

async function loadBadges() {
  try {
    badges.value = await api.checkAchievements()
  } catch {
    /* 忽略：成就非核心路径 */
  }
}

const today = computed(() => todayStr())
const dateLabel = computed(() => `${today.value} · ${weekdayLabel(today.value)}`)

const todayTodoList = computed(() =>
  todos.todos
    .filter((t) => t.dueDate <= today.value || t.status === '已逾期')
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

onMounted(async () => {
  window.addEventListener('mindmate:event', onGlobalEvent)
  nodes.date = today.value
  await Promise.all([
    nodes.load(today.value),
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
  abortBrief?.()
})

// 一键把今日待办完成
async function completeTodo(id: number) {
  await todos.toggle(id)
  await app.refreshStats()
}
</script>

<template>
  <div class="grid-today">
    <div class="col-stack">
      <!-- 晨间简报 -->
      <section class="card">
        <div class="row" style="align-items: flex-start">
          <div
            style="width: 40px; height: 40px; border-radius: 50%; background: var(--primary-weak); color: var(--primary); display: grid; place-items: center; font-size: 18px; flex: none"
          >
            📋
          </div>
          <div style="flex: 1; min-width: 0">
            <div class="card-title" style="font-size: 15px">
              {{ $t('晨间简报') }}
              <span class="card-sub">{{ briefAt ? $t('{a} 生成', { a: briefAt }) : $t('尚未生成') }}</span>
            </div>
            <div v-if="!briefContent && !briefLoading" class="small muted" style="margin-top: 4px">
              {{ $t('看看今天的安排，让智伴帮你理一理。') }}
            </div>
          </div>
          <button v-if="briefLoading" class="btn btn-sm" @click="stopBrief">{{ $t('停止') }}</button>
          <button v-else class="btn btn-sm btn-primary" @click="generateBrief">
            {{ briefContent ? $t('重新生成') : $t('生成简报') }}
          </button>
          <button v-if="briefContent" class="icon-btn" @click="briefOpen = !briefOpen">
            {{ briefOpen ? '▾' : '▸' }}
          </button>
        </div>
        <div v-if="briefOpen && (briefContent || briefLoading)" style="margin-top: 12px">
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
      <QuickEntry :date="today" autofocus />

      <!-- 时间线 -->
      <section class="card">
        <div class="row" style="margin-bottom: 10px">
          <div class="card-title" style="font-size: 15px">{{ $t('今日记录') }}</div>
          <span class="card-sub">{{ $t('{a} 条', { a: nodes.nodes.length }) }}</span>
          <div class="spacer"></div>
          <span class="small muted">{{ $t('按时刻排列 · 一次录入即一个节点') }}</span>
        </div>
        <div v-if="nodes.nodes.length === 0" class="empty">
          <div class="ill">📝</div>
          <div class="t">{{ $t('今天还没有记录') }}</div>
          <div class="d">{{ $t('记下第一笔，10 秒就好') }}</div>
        </div>
        <div v-else class="tl">
          <NodeItem
            v-for="(n, i) in nodes.nodes"
            :key="n.id"
            :node="n"
            :show-line="i < nodes.nodes.length - 1"
          />
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
        <TodoItem v-for="t in todayTodoList" :key="t.id" :todo="t" />
        <button class="btn" style="width: 100%; justify-content: center; margin-top: 8px" @click="showTodoModal = true">
          {{ $t('＋ 添加今日待办') }}
        </button>
      </section>

      <!-- 逾期提醒 -->
      <section v-if="todos.overdue.length" class="card">
        <div class="card-title" style="font-size: 15px; color: var(--danger)">{{ $t('⚠️ 已逾期 {a}', { a: todos.overdue.length }) }}</div>
        <div style="margin-top: 8px">
          <TodoItem v-for="t in todos.overdue.slice(0, 5)" :key="t.id" :todo="t" :show-actions="false" />
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
      @close="showTodoModal = false"
      @saved="showTodoModal = false; todos.load(); app.refreshStats()"
    />
  </div>
</template>
