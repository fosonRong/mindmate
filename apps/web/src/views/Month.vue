<script setup lang="ts">
// 月视图：月历网格按日聚合 + 月度进度 + 日详情抽屉（含补录）
import { computed, onMounted, ref } from 'vue'
import { useAppStore, todayStr, addDays, monthRange, fmtDate, monthTitle, weekdayLabel } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { useTodosStore } from '@/stores/todos'
import { api } from '@/api/client'
import type { MonthlySummary, Node, PeriodStats } from '@/api/types'
import ProgressPair from '@/components/ProgressPair.vue'
import CalendarMonth from '@/components/CalendarMonth.vue'
import QuickEntry from '@/components/QuickEntry.vue'
import TodoItem from '@/components/TodoItem.vue'
import { daySubLabel, dayBadge } from '@/lib/lunar'
import { t } from '@/i18n'

const app = useAppStore()
const nodes = useNodesStore()
const todos = useTodosStore()

const lunarOf = (date: string) => daySubLabel(date)
const badgeOf = (date: string) => dayBadge(date)

const anchor = ref(todayStr())
const stats = ref<PeriodStats | null>(null)
const summary = ref<MonthlySummary | null>(null)
const drawerDate = ref<string | null>(null)

const daysInMonth = computed(() => {
  const d = new Date(anchor.value.replace(/-/g, '/'))
  return new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate()
})

const projectGoal = computed(() => (app.stats?.dailyGoal ?? 4) * daysInMonth.value)

async function load() {
  const [from, to] = monthRange(anchor.value)
  const [period, monthSummary] = await Promise.all([
    api.periodStats(from, to),
    api.monthlySummary(anchor.value).catch(() => null),
    nodes.loadRange(from, to),
    todos.load() // 日历格需要展示待办标题
  ])
  stats.value = period
  summary.value = monthSummary
}

/** 月份筛选（input[type=month]） */
function pickMonth(e: Event) {
  const v = (e.target as HTMLInputElement).value
  if (!v) return
  anchor.value = `${v}-01`
  load()
}

/** 本月每日待办标题（日历格内展示） */
const todoTitles = computed(() => {
  const map: Record<string, string[]> = {}
  todos.todos
    .filter((t) => t.status !== '已完成')
    .forEach((t) => {
      if (!map[t.dueDate]) map[t.dueDate] = []
      map[t.dueDate].push(t.dueTime ? `${t.dueTime} ${t.title}` : t.title)
    })
  return map
})

const todoCounts = computed(() => {
  const map: Record<string, number> = {}
  todos.todos.forEach((t) => {
    if (t.status === '已完成') return
    map[t.dueDate] = (map[t.dueDate] || 0) + 1
  })
  return map
})

function shiftMonth(n: number) {
  const d = new Date(anchor.value.replace(/-/g, '/'))
  d.setMonth(d.getMonth() + n)
  anchor.value = fmtDate(d)
  load()
}

async function openDay(date: string) {
  drawerDate.value = date
  await Promise.all([nodes.load(date), todos.loadSchedule(date)])
}

function closeDrawer() {
  drawerDate.value = null
  nodes.load(todayStr())
}

const drawerNodes = computed<Node[]>(() => nodes.nodes)

async function onBackfillSaved() {
  await load()
}

onMounted(load)
</script>

<template>
  <div class="col-stack">
    <section class="card">
      <div class="row" style="margin-bottom: 10px">
        <div class="card-title" style="font-size: 15px">{{ $t('月度进度') }}</div>
        <span class="card-sub">{{ monthTitle(anchor) }}</span>
        <div class="spacer"></div>
        <span class="small muted">{{ $t('已录 {a}/{b} 条', { a: stats?.nodeCount ?? 0, b: projectGoal }) }}</span>
        <button class="icon-btn" :title="$t('上个月')" @click="shiftMonth(-1)">◀</button>
        <input
          type="month"
          class="input"
          style="width: 140px; height: 30px; font-size: 12px"
          :value="anchor.slice(0, 7)"
          @change="pickMonth"
        />
        <button class="btn btn-sm" @click="anchor = todayStr(); load()">{{ $t('本月') }}</button>
        <button class="icon-btn" :title="$t('下个月')" @click="shiftMonth(1)">▶</button>
      </div>
      <ProgressPair
        :node-count="stats?.nodeCount ?? 0"
        :daily-goal="projectGoal"
        :goal-enabled="true"
        :goal-label="`${stats?.nodeCount ?? 0}/${projectGoal}`"
        :todo-done="stats?.doneTodos ?? 0"
        :todo-total="stats?.totalTodos ?? 0"
      />
      <div class="small muted" style="margin-top: 8px">
        {{ $t('有录入 {a}/{b} 天 · 格内显示当日记录摘要与待办标题 · 点击任意日期查看或补录', { a: stats?.daysWithRecords ?? 0, b: stats?.totalDays ?? daysInMonth }) }}
      </div>
    </section>

    <!-- 月度小结（FR-6.4）：本月记录 X 天 / 完成待办 Y 件 / 连续最长 Z 天 -->
    <section v-if="summary" class="card" :class="{ 'month-end': summary.isMonthEnd }">
      <div class="row" style="margin-bottom: 10px">
        <div class="card-title" style="font-size: 15px">{{ $t('本月小结') }}</div>
        <span class="card-sub">{{ summary.month }}</span>
        <div class="spacer"></div>
        <span v-if="summary.isMonthEnd" class="badge ok">{{ $t('月初至今已收官') }}</span>
      </div>
      <div class="summary-grid">
        <div class="summary-item">
          <div class="num">{{ summary.daysWithRecords }}<span class="unit">{{ $t('/{a} 天', { a: summary.totalDays }) }}</span></div>
          <div class="lbl">{{ $t('本月记录天数') }}</div>
        </div>
        <div class="summary-item">
          <div class="num">{{ summary.doneTodos }}<span class="unit">{{ $t('/{a} 件', { a: summary.totalTodos }) }}</span></div>
          <div class="lbl">{{ $t('完成待办') }}</div>
        </div>
        <div class="summary-item">
          <div class="num">{{ summary.longestStreak }}<span class="unit">{{ $t('天') }}</span></div>
          <div class="lbl">{{ $t('连续最长') }}</div>
        </div>
        <div class="summary-item">
          <div class="num">{{ summary.nodeCount }}<span class="unit">{{ $t('条') }}</span></div>
          <div class="lbl">{{ $t('记录节点 · 日均 {a}', { a: summary.avgPerActiveDay }) }}</div>
        </div>
      </div>
    </section>

    <section class="card">
      <CalendarMonth
        :month-date="anchor"
        :days="stats?.days || []"
        :selected="drawerDate || undefined"
        :todo-counts="todoCounts"
        :todo-titles="todoTitles"
        :max-todo-titles="2"
        @select="openDay"
        @drop-todo="(p) => todos.reschedule(p.id, p.date).then(() => app.toast('success', t('已改期至 {a}', { a: p.date })))"
      />
    </section>

    <!-- 日详情抽屉 -->
    <div v-if="drawerDate">
      <div class="drawer-mask" @click="closeDrawer"></div>
      <aside class="drawer">
        <div class="row" style="margin-bottom: 12px">
          <div>
            <div class="card-title" style="font-size: 15px">{{ drawerDate }}</div>
            <div class="small muted">
              {{ $t('{a} · {b} 条记录', { a: weekdayLabel(drawerDate), b: drawerNodes.length }) }}
              <span v-if="lunarOf(drawerDate)"> · {{ lunarOf(drawerDate) }}</span>
              <span v-if="badgeOf(drawerDate)" class="day-badge" :class="badgeOf(drawerDate) === '休' ? 'off' : 'work'" style="margin-left: 4px">{{ badgeOf(drawerDate) }}</span>
            </div>
          </div>
          <div class="spacer"></div>
          <button class="icon-btn" @click="closeDrawer">×</button>
        </div>

        <QuickEntry :date="drawerDate" compact @saved="onBackfillSaved" />

        <div style="flex: 1; overflow-y: auto; margin-top: 12px">
          <div v-if="drawerNodes.length === 0" class="empty" style="padding: 30px 0">
            <div class="ill">🕐</div>
            <div class="t">{{ $t('这一天还没有记录') }}</div>
            <div class="d">{{ $t('补录的内容会标记为「补录」') }}</div>
          </div>
          <div v-else class="tl">
            <div v-for="(n, i) in drawerNodes" :key="n.id" class="tl-item" :class="{ backfill: n.isBackfill }">
              <div class="tl-time mono">{{ n.createdAt.slice(11, 16) }}</div>
              <div class="tl-rail">
                <div class="tl-dot"></div>
                <div v-if="i < drawerNodes.length - 1" class="tl-line"></div>
              </div>
              <div class="tl-bubble">
                <div class="tl-meta">
                  <span v-for="t in n.tags" :key="t" class="chip work">{{ t }}</span>
                  <span v-if="n.isBackfill" class="small muted">{{ $t('补录') }}</span>
                </div>
                <div class="tl-content">{{ n.content }}</div>
              </div>
            </div>
          </div>

          <div v-if="todos.schedule && (todos.schedule.schedules.length || todos.schedule.todos.length)" style="margin-top: 16px">
            <div class="card-title" style="font-size: 14px; margin-bottom: 8px">{{ $t('该日待办') }}</div>
            <TodoItem v-for="t in todos.schedule.todos" :key="t.id" :todo="t" />
          </div>
        </div>
      </aside>
    </div>
  </div>
</template>
