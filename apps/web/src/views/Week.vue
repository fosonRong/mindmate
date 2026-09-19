<script setup lang="ts">
// 周视图：周一~周日按日聚合 + 周进度 + 农历/节假日（休/班）
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue'
import { useAppStore, weekStart, addDays, fmtDate, weekdayLabel, parseDate } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { api } from '@/api/client'
import type { PeriodStats } from '@/api/types'
import ProgressPair from '@/components/ProgressPair.vue'
import { daySubLabel, dayBadge } from '@/lib/lunar'
import { t } from '@/i18n'

const app = useAppStore()
const nodes = useNodesStore()

const anchor = ref(fmtDate(new Date()))
const stats = ref<PeriodStats | null>(null)
const loading = ref(false)

const from = computed(() => weekStart(anchor.value))
const to = computed(() => addDays(from.value, 6))

const goalTotal = computed(() => {
  const goal = app.stats?.dailyGoal ?? 4
  return goal * 7
})

const days = computed(() => {
  const list: { date: string; label: string; isToday: boolean; isFuture: boolean }[] = []
  const today = fmtDate(new Date())
  for (let i = 0; i < 7; i++) {
    const d = addDays(from.value, i)
    list.push({
      date: d,
      label: weekdayLabel(d),
      isToday: d === today,
      isFuture: d > today
    })
  }
  return list
})

function statOf(date: string) {
  return stats.value?.days.find((x) => x.date === date)
}

/** 周末或法定休日 → 日期标红 */
function isRedDay(date: string) {
  const dow = parseDate(date).getDay()
  return dow === 0 || dow === 6
}

// 内容自适应（用户反馈：明明还有大片空间却只显示 3 条，不合适）：
// 默认**全量渲染**完整文本（CSS overflow 裁剪兜底），用测量判断「真放不下」
// 才出「＋N 更多」；放得下就全部展示、不出按钮。展开态走卡内滚动。
// 测量时机会踩坑：初次渲染的某一帧卡片高度还没被网格撑开，量出「全折叠」的假结果，
// 所以除了数据加载后量一次，还逐格挂 ResizeObserver（初始回调 + 尺寸变化都会触发）、
// 字体就绪后再量一次，保证自愈。
const expanded = ref<Record<string, boolean>>({})
const hiddenCount = ref<Record<string, number>>({})
const bodyEls: Record<string, HTMLElement | null> = {}
const bodyResize: Record<string, ResizeObserver> = {}
let raf1 = 0
let raf2 = 0

function isExpanded(date: string) {
  return expanded.value[date] === true
}

function toggleExpand(date: string) {
  expanded.value[date] = !isExpanded(date)
  nextTick(measureAll)
}

function setBodyRef(date: string) {
  return (el: unknown) => {
    bodyEls[date] = (el as HTMLElement) || null
  }
}

/** 量单格：数出被裁掉（底边超出可视区，含 4px 容差）的条数 */
function measureOne(date: string) {
  const el = bodyEls[date]
  if (!el || isExpanded(date)) {
    hiddenCount.value[date] = 0
    return
  }
  const bodyTop = el.getBoundingClientRect().top
  const limit = el.clientHeight + 4
  let hidden = 0
  for (const child of Array.from(el.children)) {
    const r = (child as HTMLElement).getBoundingClientRect()
    if (r.height > 0 && r.bottom - bodyTop > limit) hidden++
  }
  hiddenCount.value[date] = hidden
}

function measureAll() {
  for (const d of days.value) measureOne(d.date)
}

/** 布局稳定后重测：连两帧 rAF + 字体就绪各兜一次（首帧测量过早的假溢出会被覆盖） */
function remeasureSettled() {
  cancelAnimationFrame(raf1)
  cancelAnimationFrame(raf2)
  raf1 = requestAnimationFrame(() => {
    measureAll()
    raf2 = requestAnimationFrame(measureAll)
  })
}

async function load() {
  loading.value = true
  try {
    const [nodesList, period] = await Promise.all([
      api.nodesRange(from.value, to.value),
      api.periodStats(from.value, to.value)
    ])
    nodes.rangeCache[`${from.value}~${to.value}`] = nodesList
    stats.value = period
  } finally {
    loading.value = false
  }
  await nextTick()
  measureAll()
  remeasureSettled()
}

function shiftWeek(n: number) {
  anchor.value = addDays(anchor.value, n * 7)
  expanded.value = {}
  load()
}

function nodesOf(date: string) {
  return (nodes.rangeCache[`${from.value}~${to.value}`] || []).filter((n) => n.date === date)
}

function ringDash(percent: number, r = 14) {
  const c = 2 * Math.PI * r
  return `${(c * percent) / 100} ${c}`
}

function ringColor(percent: number) {
  return percent > 70 ? 'var(--success)' : percent >= 30 ? 'var(--warning)' : 'var(--text-disable)'
}

function recordLabel() {
  if (!stats.value) return ''
  const goal = goalTotal.value
  return t('{a}/{b} 条', { a: stats.value.nodeCount, b: goal })
}

onMounted(async () => {
  await load()
  // 逐格观察：格子尺寸随窗口/网格变化时重测（观察本身也会触发一次初始回调）
  if (typeof ResizeObserver !== 'undefined') {
    for (const d of days.value) {
      const el = bodyEls[d.date]
      if (!el) continue
      const ro = new ResizeObserver(() => measureOne(d.date))
      ro.observe(el)
      bodyResize[d.date] = ro
    }
  }
  // 中文字体晚于首帧就绪会让换行变化，就绪后补测一次
  if (document.fonts?.ready) document.fonts.ready.then(() => measureAll()).catch(() => {})
})

onUnmounted(() => {
  Object.values(bodyResize).forEach((ro) => ro.disconnect())
  cancelAnimationFrame(raf1)
  cancelAnimationFrame(raf2)
})
</script>

<template>
  <div class="col-stack">
    <!-- 周汇总 -->
    <section class="card">
      <div class="row" style="margin-bottom: 10px">
        <div class="card-title" style="font-size: 15px">{{ $t('本周进度') }}</div>
        <span class="card-sub">{{ from }} ~ {{ to }}</span>
        <div class="spacer"></div>
        <span v-if="app.stats && app.stats.streakDays > 0" class="streak">{{ $t('🔥 连续 {a} 天', { a: app.stats.streakDays }) }}</span>
        <button class="icon-btn" :title="$t('上一周')" @click="shiftWeek(-1)">◀</button>
        <button class="btn btn-sm" @click="anchor = fmtDate(new Date()); load()">{{ $t('本周') }}</button>
        <button class="icon-btn" :title="$t('下一周')" @click="shiftWeek(1)">▶</button>
      </div>
      <ProgressPair
        :node-count="stats?.nodeCount ?? 0"
        :daily-goal="goalTotal"
        :goal-enabled="true"
        :goal-label="recordLabel()"
        :todo-done="stats?.doneTodos ?? 0"
        :todo-total="stats?.totalTodos ?? 0"
      />
      <div class="small muted" style="margin-top: 8px">
        {{ $t('有录入 {a}/7 天 · 共 {b} 条记录', { a: stats?.daysWithRecords ?? 0, b: stats?.nodeCount ?? 0 }) }}
      </div>
    </section>

    <!-- 7 列：卡片宽高等额自适应，内容超出隐藏（展开态改为卡内滚动，不撑大布局） -->
    <div class="week-grid">
      <div
        v-for="d in days"
        :key="d.date"
        class="card week-card"
        :class="{ 'today-card': d.isToday, 'red-day': isRedDay(d.date) }"
        :style="d.isFuture ? 'opacity:.55' : d.isToday ? 'border-color:var(--primary)' : ''"
      >
        <div class="row" style="justify-content: space-between; margin-bottom: 2px">
          <b style="font-size: 14px">{{ d.label }}</b>
          <span class="mono small muted">{{ d.date.slice(5) }}</span>
        </div>
        <div class="row week-lunar" style="margin-bottom: 6px">
          <span class="small muted lunar-text">{{ daySubLabel(d.date) }}</span>
          <span v-if="dayBadge(d.date)" class="day-badge" :class="dayBadge(d.date) === '休' ? 'off' : 'work'">{{ dayBadge(d.date) }}</span>
        </div>

        <template v-if="statOf(d.date) && statOf(d.date)!.nodeCount > 0">
          <div class="row" style="margin-bottom: 6px">
            <span class="badge info">{{ $t('{a} 条', { a: statOf(d.date)!.nodeCount }) }}</span>
            <div class="spacer"></div>
            <div class="ring" style="width: 34px; height: 34px">
              <svg width="34" height="34">
                <circle cx="17" cy="17" r="14" fill="none" stroke="var(--bg-hover)" stroke-width="4" />
                <circle
                  cx="17"
                  cy="17"
                  r="14"
                  fill="none"
                  :stroke="ringColor(statOf(d.date)!.totalTodos ? Math.round((statOf(d.date)!.doneTodos / statOf(d.date)!.totalTodos) * 100) : 0)"
                  stroke-width="4"
                  stroke-linecap="round"
                  :stroke-dasharray="ringDash(statOf(d.date)!.totalTodos ? (statOf(d.date)!.doneTodos / statOf(d.date)!.totalTodos) * 100 : 0)"
                />
              </svg>
              <span class="num">{{ statOf(d.date)!.doneTodos }}/{{ statOf(d.date)!.totalTodos }}</span>
            </div>
          </div>
          <div class="week-body" :class="{ open: isExpanded(d.date), folded: !isExpanded(d.date) && hiddenCount[d.date] > 0 }" :ref="setBodyRef(d.date)">
            <div
              v-for="n in nodesOf(d.date)"
              :key="n.id"
              class="small week-node-line"
            >
              <span class="mono muted">{{ n.createdAt.slice(11, 16) }}</span>
              {{ n.content }}
            </div>
          </div>
          <button
            v-if="isExpanded(d.date) || hiddenCount[d.date] > 0"
            class="btn-more"
            :title="isExpanded(d.date) ? $t('收起 ▴') : $t('展开当天全部记录')"
            @click.stop="toggleExpand(d.date)"
          >
            {{ isExpanded(d.date) ? $t('收起 ▴') : $t('＋{a} 更多', { a: hiddenCount[d.date] }) }}
          </button>
        </template>

        <template v-else-if="d.isFuture">
          <div class="small muted">{{ $t('未来') }}</div>
        </template>
        <template v-else>
          <div class="small muted">{{ $t('这天还没有记录') }}</div>
          <div class="small muted" style="margin-top: 2px; opacity: 0.75">{{ $t('月视图点这天可补录') }}</div>
        </template>
      </div>
    </div>
  </div>
</template>
