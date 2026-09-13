<script setup lang="ts">
// 双进度条（记录进度 + 待办完成率），周期卡片共用
const props = defineProps<{
  nodeCount: number
  dailyGoal: number
  goalEnabled?: boolean
  goalLabel?: string
  todoDone: number
  todoTotal: number
}>()

function cls(percent: number) {
  return percent > 70 ? 'p-high' : percent >= 30 ? 'p-mid' : 'p-low'
}
function pct(done: number, total: number) {
  return total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0
}

const recordPct = () => {
  if (!props.goalEnabled) return 0
  return pct(props.nodeCount, props.dailyGoal)
}
</script>

<template>
  <div class="progress-pair">
    <div class="row">
      <span class="label muted small">记录进度</span>
      <div class="progress" :class="cls(recordPct())" style="flex: 1">
        <i :style="{ width: recordPct() + '%' }"></i>
      </div>
      <span class="val small muted" style="width: 74px; text-align: right">
        {{ goalLabel || `${nodeCount}/${dailyGoal} 条` }}
      </span>
    </div>
    <div class="row">
      <span class="label muted small">待办完成</span>
      <div class="progress" :class="cls(pct(todoDone, todoTotal))" style="flex: 1">
        <i :style="{ width: pct(todoDone, todoTotal) + '%' }"></i>
      </div>
      <span class="val small muted" style="width: 74px; text-align: right">{{ todoDone }}/{{ todoTotal }}</span>
    </div>
  </div>
</template>
