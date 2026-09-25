<script setup lang="ts">
// 待办详情弹窗（v1.2.2）：点击待办行展示完整信息 + 快捷操作。
// 只读详情（编辑仍走 TodoEditModal），描述/提醒/循环/标签/时间线一屏看全。
import { computed, onMounted, ref } from 'vue'
import type { Todo } from '@/api/types'
import { friendlyDate, weekdayLabel } from '@/stores/app'
import { api } from '@/api/client'
import { useTodosStore } from '@/stores/todos'
import { useAppStore } from '@/stores/app'
import { t } from '@/i18n'

const props = defineProps<{ todo: Todo }>()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'edit', todo: Todo): void
  (e: 'pin', todo: Todo): void
  (e: 'toggle', id: number): void
}>()

const todos = useTodosStore()
const app = useAppStore()

// ── 相关事项（v1.4.1）：标签/关键词/时间邻近串联 ──
const related = ref<{ nodes: { id: number; title: string; date: string }[]; todos: { id: number; title: string; date: string }[] } | null>(null)
const relatedLoading = ref(false)

async function loadRelated() {
  if (props.todo.inbox) return
  relatedLoading.value = true
  try {
    related.value = await api.relatedItems('todo', props.todo.id, 4)
  } catch {
    related.value = null
  } finally {
    relatedLoading.value = false
  }
}

function jumpToRelated(kind: string, id: number) {
  if (kind === 'todo') {
    location.hash = '#/todos'
  } else {
    // 记录 → 月视图并打开该日抽屉（复用命令面板事件）
    window.dispatchEvent(new CustomEvent('mindmate:open-day', { detail: { date: props.todo.dueDate } }))
    location.hash = '#/month'
  }
  emit('close')
  void id
}

const recurLabel = computed(() => {
  const map: Record<string, string> = { daily: t('每天'), weekly: t('每周'), monthly: t('每月') }
  return map[props.todo.recurType] || props.todo.recurType
})

const remindText = computed(() => {
  if (!props.todo.remindAt) return t('未设置提醒')
  const ra = props.todo.remindAt
  const date = ra.slice(0, 10)
  const time = ra.slice(11, 16)
  if (props.todo.dueTime) {
    const [rh, rm] = time.split(':').map(Number)
    const [dh, dm] = props.todo.dueTime.split(':').map(Number)
    const diff = (dh * 60 + dm) - (rh * 60 + rm)
    if (diff === 0) return t('准点提醒 · {a}', { a: time })
    if (diff > 0 && diff <= 1440) return t('提前 {a} 分钟 · {b}', { a: diff, b: time })
  }
  return `${friendlyDate(date)} ${time}`
})

function isPinned(todo: Todo) {
  return todo.sortOrder < 0
}

onMounted(() => loadRelated().catch(() => {}))
</script>

<template>
  <div class="modal-mask" @click.self="emit('close')">
    <div class="modal todo-detail">
      <div class="row" style="align-items: flex-start">
        <h3 style="flex: 1; min-width: 0; word-break: break-word">{{ todo.title }}</h3>
        <button class="icon-btn" :title="$t('关闭')" @click="emit('close')">✕</button>
      </div>

      <div class="detail-grid">
        <div class="detail-item">
          <span class="lbl">{{ $t('状态') }}</span>
          <span class="badge" :class="todo.status === '已完成' ? 'ok' : todo.overdue ? 'danger' : 'info'">
            {{ todo.status }}{{ todo.inbox ? ' · ' + $t('收集箱') : '' }}
          </span>
        </div>
        <div class="detail-item">
          <span class="lbl">{{ $t('日期') }}</span>
          <span>{{ todo.dueDate }} · {{ weekdayLabel(todo.dueDate) }}</span>
        </div>
        <div class="detail-item" v-if="todo.dueTime">
          <span class="lbl">{{ $t('时间') }}</span>
          <span class="mono">{{ todo.dueTime }}</span>
        </div>
        <div class="detail-item">
          <span class="lbl">{{ $t('提醒') }}</span>
          <span>{{ remindText }}</span>
        </div>
        <div class="detail-item">
          <span class="lbl">{{ $t('优先级') }}</span>
          <span>{{ todo.priority }}</span>
        </div>
        <div class="detail-item" v-if="todo.recurType">
          <span class="lbl">{{ $t('循环') }}</span>
          <span>🔁 {{ recurLabel }}{{ todo.recurInterval > 1 ? ' × ' + todo.recurInterval : '' }}
            <template v-if="todo.recurUntil"> · {{ $t('截止 {a}', { a: todo.recurUntil }) }}</template>
            <template v-if="todo.recurSkipRest"> · {{ $t('休息日顺延') }}</template>
          </span>
        </div>
        <div class="detail-item" v-if="todo.tags.length">
          <span class="lbl">{{ $t('标签') }}</span>
          <span class="row wrap" style="gap: 4px">
            <span v-for="tag in todo.tags" :key="tag" class="chip">{{ tag }}</span>
          </span>
        </div>
        <div class="detail-item" v-if="todo.description">
          <span class="lbl">{{ $t('描述') }}</span>
          <span class="detail-desc">{{ todo.description }}</span>
        </div>
        <div class="detail-item">
          <span class="lbl">{{ $t('创建于') }}</span>
          <span class="mono small muted">{{ todo.createdAt.slice(0, 16) }}</span>
        </div>
      </div>

      <!-- 相关事项（v1.4.1） -->
      <div v-if="related && (related.nodes.length || related.todos.length)" class="divider" style="margin: 10px 0"></div>
      <div v-if="related && (related.nodes.length || related.todos.length)">
        <div class="small muted" style="margin-bottom: 4px">{{ $t('🔗 相关事项') }}</div>
        <div
          v-for="n in related.nodes"
          :key="'n' + n.id"
          class="small related-item"
          @click="jumpToRelated('node', n.id)"
        >
          📝 {{ n.title.slice(0, 40) }}{{ n.title.length > 40 ? '…' : '' }}
          <span class="mono muted">{{ n.date.slice(5) }}</span>
        </div>
        <div
          v-for="x in related.todos"
          :key="'t' + x.id"
          class="small related-item"
          @click="jumpToRelated('todo', x.id)"
        >
          ✅ {{ x.title.slice(0, 40) }}{{ x.title.length > 40 ? '…' : '' }}
          <span class="mono muted">{{ x.date.slice(5) }}</span>
        </div>
      </div>

      <div class="modal-actions">
        <button class="btn" :class="{ 'pin-on': isPinned(todo) }" @click="emit('pin', todo)">
          {{ isPinned(todo) ? $t('★ 取消置顶') : $t('☆ 置顶') }}
        </button>
        <button class="btn" @click="emit('toggle', todo.id)">
          {{ todo.status === '已完成' ? $t('标记未完成') : $t('标记完成') }}
        </button>
        <button class="btn btn-primary" @click="emit('edit', todo)">{{ $t('编辑') }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.todo-detail { max-width: 480px; }
.detail-grid { display: flex; flex-direction: column; gap: 8px; margin: 12px 0 4px; }
.detail-item { display: flex; gap: 12px; align-items: baseline; font-size: 13px; }
.detail-item .lbl { flex: none; width: 44px; color: var(--text-weak); font-size: 12px; }
.detail-item span:not(.lbl):not(.badge) { color: var(--text-strong); min-width: 0; word-break: break-word; }
.detail-desc { white-space: pre-wrap; line-height: 1.6; }
.pin-on { color: var(--warning); border-color: var(--warning); }
.related-item { padding: 3px 6px; border-radius: 6px; cursor: pointer; color: var(--text-regular); }
.related-item:hover { background: var(--bg-hover); color: var(--primary); }
</style>
