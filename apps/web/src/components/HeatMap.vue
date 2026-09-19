<script setup lang="ts">
// 记录热力图：近 N 周（默认 26 周）GitHub 贡献图风格，颜色深浅=当日记录数相对每日目标
// 数据来自 periodStats.days（nodeCount），点击格子跳到所在月份并打开日详情
import { computed } from 'vue'
import type { DayStat } from '@/api/types'
import { fmtDate, parseDate, addDays, todayStr } from '@/stores/app'
import { t } from '@/i18n'

const props = withDefaults(
  defineProps<{
    days: DayStat[]
    dailyGoal: number
    weeks?: number
  }>(),
  { weeks: 26, dailyGoal: 4 },
)

const emit = defineEmits<{ (e: 'pick', date: string): void }>()

const today = todayStr()

interface Cell {
  date: string
  count: number
  level: 0 | 1 | 2 | 3 | 4
  weekend: boolean
}

// 从本周（含今天）往回推 weeks 周，起点对齐到周一
const cells = computed<Cell[]>(() => {
  const map: Record<string, DayStat> = {}
  props.days.forEach((d) => (map[d.date] = d))
  const todayKey = todayStr()
  const today = parseDate(todayKey)
  // 本周起点对齐周一（周一=0）
  const weekStart = addDays(todayKey, -((today.getDay() + 6) % 7))
  const startKey = addDays(weekStart, -7 * (props.weeks - 1))
  const goal = Math.max(props.dailyGoal, 1)
  const out: Cell[] = []
  for (let i = 0; i < props.weeks * 7; i++) {
    const key = addDays(startKey, i)
    const d = parseDate(key)
    const stat = map[key]
    const count = stat?.nodeCount ?? 0
    const ratio = count / goal
    const level: Cell['level'] =
      count === 0 ? 0 : ratio < 0.5 ? 1 : ratio < 1 ? 2 : ratio <= 1.5 ? 3 : 4
    out.push({
      date: key,
      count,
      level,
      weekend: d.getDay() === 0 || d.getDay() === 6,
    })
  }
  return out
})

const totalActive = computed(() => cells.value.filter((c) => c.count > 0).length)
const totalRecords = computed(() => cells.value.reduce((s, c) => s + c.count, 0))

function tip(c: Cell) {
  const label = `${c.date} · ${t('{a} 条记录', { a: c.count })}`
  return c.count === 0 ? label : `${label} · ${t('连续记录的一部分')}`
}
</script>

<template>
  <div>
    <div class="heat-scroll">
      <div class="heat-grid">
        <div
          v-for="c in cells"
          :key="c.date"
          class="heat-cell"
          :class="[`l${c.level}`, { future: c.date > today }]"
          :title="tip(c)"
          @click="emit('pick', c.date)"
        ></div>
      </div>
    </div>
    <div class="row" style="margin-top: 8px">
      <span class="small muted">
        {{
          $t('近 {a} 周有记录 {b} 天 · 共 {c} 条', {
            a: weeks,
            b: totalActive,
            c: totalRecords,
          })
        }}
      </span>
      <div class="spacer"></div>
      <div class="heat-legend">
        <span>{{ $t('少') }}</span>
        <i></i><i class="l1"></i><i class="l2"></i><i class="l3"></i><i class="l4"></i>
        <span>{{ $t('多') }}</span>
      </div>
    </div>
  </div>
</template>
