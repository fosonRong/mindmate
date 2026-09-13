<script setup lang="ts">
// 日历（月视图），支持拖拽改期、节点数徽标、待办圆点
import { computed } from 'vue'
import type { DayStat } from '@/api/types'
import { WEEKDAYS, fmtDate, parseDate, monthRange } from '@/stores/app'

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
}

const cells = computed<Cell[]>(() => {
  const [start, end] = monthRange(props.monthDate)
  const first = parseDate(start)
  const last = parseDate(end)
  const offset = (first.getDay() + 6) % 7 // 周一为起点
  const out: Cell[] = []
  const statMap: Record<string, DayStat> = {}
  props.days.forEach((d) => (statMap[d.date] = d))

  for (let i = 0; i < offset; i++) {
    const d = new Date(first)
    d.setDate(d.getDate() - (offset - i))
    out.push({ date: fmtDate(d), day: d.getDate(), inMonth: false, isToday: false, todoCount: 0, todoTitles: [] })
  }
  const cur = new Date(first)
  while (cur <= last) {
    const key = fmtDate(cur)
    out.push({
      date: key,
      day: cur.getDate(),
      inMonth: true,
      isToday: key === today,
      stat: statMap[key],
      todoCount: props.todoCounts?.[key] || 0,
      todoTitles: props.todoTitles?.[key] || []
    })
    cur.setDate(cur.getDate() + 1)
  }
  // 补齐到 7 的倍数
  while (out.length % 7 !== 0) {
    const lastCell = out[out.length - 1]
    const d = parseDate(lastCell.date)
    d.setDate(d.getDate() + 1)
    out.push({ date: fmtDate(d), day: d.getDate(), inMonth: false, isToday: false, todoCount: 0, todoTitles: [] })
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
          'compact': compact
        }"
        @click="c.inMonth && emit('select', c.date)"
        @dragover="c.inMonth ? onDragOver($event) : null"
        @drop="c.inMonth ? onDrop($event, c.date) : null"
      >
        <div class="num">
          <span>{{ c.day }}</span>
          <span v-if="c.stat && c.stat.nodeCount > 0" class="cnt">{{ c.stat.nodeCount }}</span>
        </div>
        <div v-if="c.stat?.nodeSummaries?.length" class="sum">{{ c.stat.nodeSummaries[0] }}</div>
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
</template>
