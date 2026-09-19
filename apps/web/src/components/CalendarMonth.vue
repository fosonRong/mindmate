<script setup lang="ts">
// 日历（月视图），支持拖拽改期、节点数徽标、待办圆点、农历与法定节假日（休/班）
import { computed } from 'vue'
import type { DayStat } from '@/api/types'
import { WEEKDAYS, fmtDate, parseDate, monthRange } from '@/stores/app'
import { daySubLabel, dayBadge, holidayInfo } from '@/lib/lunar'

const props = defineProps<{
  monthDate: string
  days: DayStat[]
  selected?: string
  todoCounts?: Record<string, number>
  /** 每日待办标题（用于在格内展示标题而非仅圆点） */
  todoTitles?: Record<string, string[]>
  /** 格内最多显示几条待办标题 */
  maxTodoTitles?: number
  compact?: boolean
}>()
const emit = defineEmits<{
  (e: 'select', date: string): void
  (e: 'drop-todo', payload: { id: number; date: string }): void
}>()

const today = fmtDate(new Date())

interface Cell {
  date: string
  day: number
  inMonth: boolean
  isToday: boolean
  stat?: DayStat
  todoCount: number
  todoTitles: string[]
  /** 农历/节日副标签（如「八月初九」「春节」「国庆节」） */
  lunar: string
  /** 法定节假日徽标：休 / 班 */
  badge: '休' | '班' | null
  /** 周末或法定休日：日期数字标红 */
  redDay: boolean
}

const cells = computed<Cell[]>(() => {
  const [start, end] = monthRange(props.monthDate)
  const first = parseDate(start)
  const last = parseDate(end)
  const offset = (first.getDay() + 6) % 7 // 周一为起点
  const out: Cell[] = []
  const statMap: Record<string, DayStat> = {}
  props.days.forEach((d) => (statMap[d.date] = d))

  const build = (date: string, day: number, inMonth: boolean): Cell => {
    const dow = parseDate(date).getDay()
    const hol = holidayInfo(date)
    return {
      date,
      day,
      inMonth,
      isToday: date === today,
      todoCount: props.todoCounts?.[date] || 0,
      todoTitles: props.todoTitles?.[date] || [],
      lunar: daySubLabel(date),
      badge: dayBadge(date),
      redDay: dow === 0 || dow === 6 || hol.kind === 'off',
    }
  }

  for (let i = 0; i < offset; i++) {
    const d = new Date(first)
    d.setDate(d.getDate() - (offset - i))
    out.push(build(fmtDate(d), d.getDate(), false))
  }
  const cur = new Date(first)
  while (cur <= last) {
    const key = fmtDate(cur)
    const c = build(key, cur.getDate(), true)
    c.stat = statMap[key]
    out.push(c)
    cur.setDate(cur.getDate() + 1)
  }
  // 补齐到 7 的倍数
  while (out.length % 7 !== 0) {
    const lastCell = out[out.length - 1]
    const d = parseDate(lastCell.date)
    d.setDate(d.getDate() + 1)
    out.push(build(fmtDate(d), d.getDate(), false))
  }
  return out
})

function onDragOver(e: DragEvent) {
  e.preventDefault()
  if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
}

function onDrop(e: DragEvent, date: string) {
  e.preventDefault()
  const raw = e.dataTransfer?.getData('text/mindmate-todo')
  if (!raw) return
  emit('drop-todo', { id: Number(raw), date })
}
</script>

<template>
  <div>
    <div class="cal-head">
      <div v-for="(w, i) in WEEKDAYS" :key="i">{{ w }}</div>
    </div>
    <div class="cal-grid">
      <div
        v-for="c in cells"
        :key="c.date"
        class="cal-cell"
        :class="{
          'other': !c.inMonth,
          'today': c.isToday,
          'selected': selected === c.date,
          'compact': compact,
          'red-day': c.redDay
        }"
        :title="c.lunar ? `${c.date} · ${c.lunar}` : c.date"
        @click="c.inMonth && emit('select', c.date)"
        @dragover="c.inMonth ? onDragOver($event) : null"
        @drop="c.inMonth ? onDrop($event, c.date) : null"
      >
        <!-- 日期 + 农历同一行（横向布局），徽标/计数靠右 -->
        <div class="num">
          <span>{{ c.day }}</span>
          <span class="lunar" :title="c.lunar">{{ c.lunar }}</span>
          <span v-if="c.badge" class="day-badge" :class="c.badge === '休' ? 'off' : 'work'">{{ c.badge }}</span>
          <span v-if="c.stat && c.stat.nodeCount > 0" class="cnt">{{ c.stat.nodeCount }}</span>
        </div>
        <!-- 日程文字区：填满格子剩余高宽，随格子伸缩，显示不下自动隐藏 -->
        <div class="sum-wrap">
          <div v-for="(s, si) in c.stat?.nodeSummaries || []" :key="`s${si}`" class="sum" :title="s">{{ s }}</div>
          <!-- 待办标题（日程）：最多显示 maxTodoTitles 条，其余以 +n 归纳 -->
          <div
            v-for="(t, i) in c.todoTitles.slice(0, maxTodoTitles ?? 2)"
            :key="i"
            class="sum todo-title"
            :title="t"
          >
            · {{ t }}
          </div>
          <div v-if="c.todoTitles.length > (maxTodoTitles ?? 2)" class="sum muted">
            {{ $t('+{a} 项待办', { a: c.todoTitles.length - (maxTodoTitles ?? 2) }) }}
          </div>
          <div v-if="!c.todoTitles.length && c.todoCount" class="todo-dots">
            <span v-for="i in Math.min(c.todoCount, 3)" :key="i"></span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
