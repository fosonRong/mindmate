<script setup lang="ts">
// 周视图：周一~周日按日聚合 + 周进度
import { computed, onMounted, ref } from 'vue'
import { useAppStore, weekStart, addDays, fmtDate, weekdayLabel, parseDate } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { api } from '@/api/client'
import type { PeriodStats } from '@/api/types'
import ProgressPair from '@/components/ProgressPair.vue'
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

// 每天默认只列 3 条；「＋N 更多」可点击展开全部（再点收起）。
// 真机踩过：这里原来只是一行纯文本提示，点击没反应——用户看到"更多"就会去点。
const FOLD_LIMIT = 3
const expanded = ref<Record<string, boolean>>({})

function isExpanded(date: string) {
  return expanded.value[date] === true
}

function toggleExpand(date: string) {
  expanded.value[date] = !isExpanded(date)
}

/** 折叠时取前 N 条，展开时取全部 */
function visibleNodes(date: string) {
  const list = nodesOf(date)
  return isExpanded(date) ? list : list.slice(0, FOLD_LIMIT)
}

/** 折叠时截断长文本，展开后显示完整内容（否则"展开"看不到更多信息） */
function displayText(content: string, date: string) {
  if (isExpanded(date)) return content
  return content.length > 22 ? `${content.slice(0, 22)}…` : content
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
}

function shiftWeek(n: number) {
  anchor.value = addDays(anchor.value, n * 7)
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

onMounted(load)
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

    <!-- 7 列 -->
    <div class="week-grid">
      <div
        v-for="d in days"
        :key="d.date"
        class="card"
        :class="{ 'today-card': d.isToday }"
        :style="d.isFuture ? 'opacity:.55' : d.isToday ? 'border-color:var(--primary)' : ''"
      >
        <div class="row" style="justify-content: space-between; margin-bottom: 6px">
          <b style="font-size: 14px">{{ d.label }}</b>
          <span class="mono small muted">{{ d.date.slice(5) }}</span>
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
          <div
            v-for="n in visibleNodes(d.date)"
            :key="n.id"
            class="small"
            style="color: var(--text-regular); margin-bottom: 2px"
          >
            <span class="mono muted">{{ n.createdAt.slice(11, 16) }}</span>
            {{ displayText(n.content, d.date) }}
          </div>
          <button
            v-if="nodesOf(d.date).length > FOLD_LIMIT"
            class="btn-more"
            :title="isExpanded(d.date) ? $t('收起 ▴') : $t('展开当天全部记录')"
            @click.stop="toggleExpand(d.date)"
          >
            {{ isExpanded(d.date) ? $t('收起 ▴') : $t('＋{a} 更多', { a: nodesOf(d.date).length - FOLD_LIMIT }) }}
          </button>
        </template>

        <template v-else-if="d.isFuture">
          <div class="small muted">{{ $t('未来') }}</div>
        </template>
        <template v-else>
          <div class="small muted">{{ $t('未记录') }}</div>
        </template>
      </div>
    </div>
  </div>
</template>
