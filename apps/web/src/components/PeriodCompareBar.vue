<script setup lang="ts">
// 报告回顾对比条（v1.2.0）：本期 vs 上期关键数字，5 秒看懂「比上期怎么样」。
// 涨跌语义：记录/完成/活跃「涨=好」；上期为 0 视为「首期」不显示涨跌。
import { computed } from 'vue'
import type { PeriodCompare, PeriodBrief } from '@/api/types'
import { t } from '@/i18n'

const props = defineProps<{ compare: PeriodCompare | null; kind: 'daily' | 'weekly' }>()

interface Row {
  label: string
  cur: string
  prev: string
  delta: number | null // 百分比；null=首期
  good: number | null // 1 涨好 / -1 跌好（完成率比较用绝对差）
}

const rows = computed<Row[]>(() => {
  const c = props.compare
  if (!c) return []
  const pct = (cur: number, prev: number): number | null => {
    if (prev <= 0) return null
    return Math.round(((cur - prev) / prev) * 100)
  }
  const rate = (b: PeriodBrief) => (b.totalTodos > 0 ? Math.round((b.doneTodos / b.totalTodos) * 100) : -1)
  const curRate = rate(c.cur)
  const prevRate = rate(c.prev)
  return [
    {
      label: props.kind === 'weekly' ? t('记录（本周）') : t('记录（今天）'),
      cur: `${c.cur.nodeCount} ${t('条')}`,
      prev: `${c.prev.nodeCount} ${t('条')}`,
      delta: pct(c.cur.nodeCount, c.prev.nodeCount),
      good: 1
    },
    {
      label: t('完成待办'),
      cur: `${c.cur.doneTodos}/${c.cur.totalTodos}`,
      prev: `${c.prev.doneTodos}/${c.prev.totalTodos}`,
      delta: pct(c.cur.doneTodos, c.prev.doneTodos),
      good: 1
    },
    {
      label: t('完成率'),
      cur: curRate < 0 ? '—' : `${curRate}%`,
      prev: prevRate < 0 ? '—' : `${prevRate}%`,
      delta: curRate < 0 || prevRate < 0 ? null : curRate - prevRate,
      good: 1
    },
    {
      label: t('活跃天数'),
      cur: `${c.cur.daysWithRecords} ${t('天')}`,
      prev: `${c.prev.daysWithRecords} ${t('天')}`,
      delta: pct(c.cur.daysWithRecords, c.prev.daysWithRecords),
      good: 1
    }
  ]
})

const periodLabel = computed(() =>
  props.kind === 'weekly' ? t('对比上周') : t('对比昨天')
)
</script>

<template>
  <div v-if="compare" class="compare-bar">
    <div class="compare-head small muted">
      {{ periodLabel }}（{{ compare.prevFrom.slice(5) }}{{ kind === 'weekly' ? '~' + compare.prevTo.slice(5) : '' }}）
    </div>
    <div class="compare-grid">
      <div v-for="r in rows" :key="r.label" class="compare-item">
        <div class="lbl">{{ r.label }}</div>
        <div class="num">
          {{ r.cur }}
          <span v-if="r.delta !== null" class="delta" :class="{ up: r.delta > 0, down: r.delta < 0 }">
            {{ r.delta > 0 ? '↑' : r.delta < 0 ? '↓' : '→' }} {{ Math.abs(r.delta) }}%
          </span>
          <span v-else-if="r.delta === null" class="delta first">{{ $t('首期') }}</span>
        </div>
        <div class="prev small muted">{{ $t('上期 {a}', { a: r.prev }) }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.compare-bar { border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; background: var(--bg-hover); }
.compare-head { margin-bottom: 6px; }
.compare-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; }
.compare-item .lbl { font-size: 11px; color: var(--text-weak); }
.compare-item .num { font-size: 14px; font-weight: 600; color: var(--text-strong); }
.delta { font-size: 11px; font-weight: 500; margin-left: 2px; }
.delta.up { color: var(--success); }
.delta.down { color: var(--danger); }
.delta.first { color: var(--text-weak); }
.compare-item .prev { font-size: 11px; }
@media (max-width: 800px) { .compare-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
