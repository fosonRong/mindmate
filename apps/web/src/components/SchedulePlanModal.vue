<script setup lang="ts">
// 智能排期弹窗（v1.5.0）：预览引擎建议 → 应用/撤销。
// 引擎只产建议，应用才写库；撤销 = 按 from_date 反向应用。
import { computed, ref } from 'vue'
import { api } from '@/api/client'
import { useTodosStore } from '@/stores/todos'
import { useAppStore } from '@/stores/app'
import { friendlyDate, todayStr } from '@/stores/app'
import { t } from '@/i18n'

interface PlanItem {
  todoId: number
  title: string
  fromDate: string
  toDate: string
  priority: string
  reason: string
}

const emit = defineEmits<{ (e: 'close'): void; (e: 'applied'): void }>()
const todos = useTodosStore()
const app = useAppStore()

const loading = ref(true)
const items = ref<PlanItem[]>([])
const dailyCapacity = ref(4)
const checked = ref<boolean[]>([])
const applying = ref(false)
const appliedOnce = ref(false)

async function load() {
  try {
    const r = await api.schedulePlanPreview()
    items.value = (r as any).items || []
    dailyCapacity.value = (r as any).dailyCapacity || 4
    checked.value = items.value.map(() => true)
  } catch (e: any) {
    app.toast('error', e?.message || t('生成失败'))
  } finally {
    loading.value = false
  }
}
load()

const movedCount = computed(() => items.value.filter((x) => x.fromDate !== x.toDate).length)
const pickedCount = computed(() => checked.value.filter(Boolean).length)

function isMoved(x: PlanItem) {
  return x.fromDate !== x.toDate
}

async function apply() {
  const picked = items.value.filter((_, i) => checked.value[i])
  if (!picked.length) return
  applying.value = true
  try {
    const r = await api.scheduleApply(picked.map((x) => ({ todoId: x.todoId, toDate: x.toDate })))
    const applied = (r as any).applied ?? 0
    app.toast('success', t('已排期 {a} 条', { a: applied }))
    appliedOnce.value = true
    emit('applied')
  } catch (e: any) {
    app.toast('error', e?.message || t('应用失败'))
  } finally {
    applying.value = false
  }
}

/** 撤销：按原日期反向应用 */
async function undo() {
  applying.value = true
  try {
    await api.scheduleApply(
      items.value
        .filter((_, i) => checked.value[i] && isMoved(items.value[i]))
        .map((x) => ({ todoId: x.todoId, toDate: x.fromDate }))
    )
    app.toast('success', t('已撤销本次排期'))
    emit('applied')
  } catch (e: any) {
    app.toast('error', e?.message || t('撤销失败'))
  } finally {
    applying.value = false
  }
}

const today = todayStr()
</script>

<template>
  <div class="modal-mask" @click.self="emit('close')">
    <div class="modal schedule-modal">
      <h3>{{ $t('🗓️ 智能排期') }}</h3>
      <div class="small muted" style="margin-bottom: 8px">
        {{
          $t('把收集箱和逾期/今日未完成的待办，按优先级与截止铺进未来工作日（每天容量 {a} 条）。应用前可逐条勾选。', { a: dailyCapacity })
        }}
      </div>

      <div v-if="loading" class="small muted">{{ $t('生成中…') }}</div>
      <template v-else>
        <div v-if="!items.length" class="empty" style="padding: 16px 0">
          <div class="t">{{ $t('没有需要排期的事项') }}</div>
          <div class="d">{{ $t('收集箱和逾期/今日未完成的待办都会出现在这里') }}</div>
        </div>
        <div v-else class="plan-list">
          <label v-for="(x, i) in items" :key="x.todoId" class="plan-row" :class="{ moved: isMoved(x), off: !checked[i] }">
            <input type="checkbox" :checked="checked[i]" @change="checked[i] = !checked[i]" />
            <span class="plan-title" :title="x.title">{{ x.title }}</span>
            <span class="plan-date mono">
              {{ isMoved(x) ? `${x.fromDate.slice(5)} → ${x.toDate.slice(5)}` : x.toDate.slice(5) }}
            </span>
            <span class="plan-reason" :title="x.reason">{{ isMoved(x) ? x.reason : $t('保持原日期') }}</span>
          </label>
        </div>
        <div class="small muted" style="margin-top: 6px">
          {{ $t('共 {a} 条 · 其中 {b} 条改变日期 · 周末不排', { a: items.length, b: movedCount }) }}
        </div>
      </template>

      <div class="modal-actions">
        <button class="btn" @click="emit('close')">{{ $t('取消') }}</button>
        <button v-if="appliedOnce" class="btn" :disabled="applying" @click="undo">{{ $t('撤销本次排期') }}</button>
        <button class="btn btn-primary" :disabled="applying || loading || !pickedCount" @click="apply">
          {{ applying ? $t('应用中…') : $t('应用（{a}）', { a: pickedCount }) }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.schedule-modal { max-width: 560px; }
.plan-list { max-height: 320px; overflow-y: auto; display: flex; flex-direction: column; gap: 4px; }
.plan-row { display: flex; align-items: center; gap: 8px; padding: 5px 8px; border-radius: 8px; background: var(--bg-hover); }
.plan-row.moved { border-left: 3px solid var(--primary); }
.plan-row.off { opacity: 0.45; }
.plan-title { flex: 1; min-width: 0; font-size: 13px; color: var(--text-strong); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.plan-date { flex: none; font-size: 12px; color: var(--text-regular); }
.plan-reason { flex: none; max-width: 40%; font-size: 11px; color: var(--text-weak); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
