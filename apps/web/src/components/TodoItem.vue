<script setup lang="ts">
// 待办项：复选框、优先级、逾期徽标、悬停操作、拖拽
import type { Todo } from '@/api/types'
import { ref } from 'vue'
import { useTodosStore } from '@/stores/todos'
import { useAppStore, friendlyDate } from '@/stores/app'
import { t } from '@/i18n'

// ⚠️ Vue 3 的 Boolean 属性「缺省即 false」：showActions 不传时是 false 而不是 undefined，
// 曾导致所有页面的待办都渲染不出删除按钮（长期 bug）。这里显式给默认值。
const props = withDefaults(
  defineProps<{ todo: Todo; draggable?: boolean; showActions?: boolean }>(),
  { draggable: false, showActions: true }
)
const emit = defineEmits<{
  (e: 'detail', todo: Todo): void
}>()
const todos = useTodosStore()
const app = useAppStore()

/** 置顶（v1.2.2）：sort_order<0 视为置顶 */
const isPinned = () => props.todo.sortOrder < 0

async function togglePin() {
  try {
    await todos.update(props.todo.id, { sortOrder: isPinned() ? 0 : -1 })
    await todos.load()
    app.toast('success', isPinned() ? t('已置顶，将显示在最前') : t('已取消置顶'))
  } catch (e: any) {
    app.toast('error', e?.message || t('操作失败'))
  }
}

function tagClass(t: string) {
  const map: Record<string, string> = { 工作: 'work', 生活: 'life', 健康: 'health', 学习: 'study' }
  return map[t] || 'none'
}

function priClass(p: string) {
  return p === '高' ? 'pri-h' : p === '中' ? 'pri-m' : 'pri-l'
}

/** 有效期展示：有开始日期显示区间，否则单日（v1.5.3） */
function dateRangeText(todo: Todo) {
  const due = friendlyDate(todo.dueDate)
  if (todo.startDate && todo.startDate !== todo.dueDate) {
    return `${todo.startDate.slice(5)} ~ ${due}`
  }
  return due
}

function recurLabel(rt: string) {
  const map: Record<string, string> = { daily: '每天', weekly: '每周', monthly: '每月' }
  return map[rt] || rt
}

function recurTip(todo: Todo) {
  const base = t('循环待办：完成后自动生成下一期')
  const skip = todo.recurSkipRest ? ' · ' + t('休息日顺延') : ''
  const until = todo.recurUntil ? ' · ' + t('截止 {a}', { a: todo.recurUntil }) : ''
  return base + skip + until
}

async function toggle() {
  try {
    await todos.toggle(props.todo.id)
    // 循环待办完成后会自动生成下一期，重拉一次让新实例立即可见
    await todos.load()
    app.refreshStats()
  } catch (e: any) {
    app.toast('error', e?.message || '操作失败')
  }
}

// 删除流程：普通待办点一次变「确认删除」再点才删（防误删）；
// 循环待办弹出两种语义（删除整个循环 / 仅删除这一条）
const confirmDelete = ref(false)
const askSeries = ref(false)
let confirmTimer: ReturnType<typeof setTimeout> | null = null

function onRequestDelete() {
  if (props.todo.recurType) {
    askSeries.value = true
    return
  }
  if (confirmDelete.value) {
    doRemove()
    return
  }
  confirmDelete.value = true
  if (confirmTimer) clearTimeout(confirmTimer)
  confirmTimer = setTimeout(() => (confirmDelete.value = false), 3000)
}

async function doRemove(scope?: 'series') {
  if (confirmTimer) clearTimeout(confirmTimer)
  confirmDelete.value = false
  askSeries.value = false
  try {
    await todos.remove(props.todo.id, scope)
    await todos.load()
    app.toast('info', t(scope === 'series' ? '已删除整个循环' : '已删除待办'))
  } catch (e: any) {
    app.toast('error', e?.message || '删除失败')
  }
}

function onDragStart(e: DragEvent) {
  e.dataTransfer?.setData('text/mindmate-todo', String(props.todo.id))
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}

// ── 子记录目录树（v1.5.3）：📝 徽标点击展开/收起，懒加载直属记录 ──
const subExpanded = ref(false)
const subNodes = ref<{ id: number; content: string; createdAt: string; date: string }[]>([])
const subLoading = ref(false)

async function toggleSub() {
  subExpanded.value = !subExpanded.value
  if (subExpanded.value && !subNodes.value.length) {
    subLoading.value = true
    try {
      const r = await (await import('@/api/client')).api.todoNodes(props.todo.id)
      subNodes.value = (r.items || []).map((n) => ({ id: n.id, content: n.content, createdAt: n.createdAt, date: n.date }))
    } catch {
      subNodes.value = []
    } finally {
      subLoading.value = false
    }
  }
}

async function unlinkSub(nodeId: number) {
  try {
    const api = (await import('@/api/client')).api
    await api.updateNode(nodeId, { todoId: null })
    subNodes.value = subNodes.value.filter((n) => n.id !== nodeId)
    if (!subNodes.value.length) subExpanded.value = false
    await todos.load()
    app.toast('success', t('已解除关联'))
  } catch (e: any) {
    app.toast('error', e?.message || t('操作失败'))
  }
}

function jumpSub(n: { date: string }) {
  window.dispatchEvent(new CustomEvent('mindmate:open-day', { detail: { date: n.date } }))
  location.hash = '#/month'
}

async function suggest() {
  try {
    const res = await (await import('@/api/client')).api.suggestSchedule(props.todo.id)
    app.toast(res.isAi ? 'info' : 'warning', `智伴建议：${res.suggestion}`)
  } catch (e: any) {
    app.toast('error', e?.message || '获取建议失败')
  }
}
</script>

<template>
  <div
    class="todo-row"
    :class="{ done: todo.status === '已完成' }"
    :draggable="draggable"
    @dragstart="onDragStart"
  >
    <div class="check" :class="{ done: todo.status === '已完成' }" @click.stop="toggle">
      <span v-if="todo.status === '已完成'">✓</span>
    </div>
    <div class="body" :title="$t('点击查看详情')" @click.stop="emit('detail', props.todo)">
      <div class="title">{{ todo.title }}</div>
      <div v-if="todo.description" class="desc" :title="todo.description">{{ todo.description }}</div>
      <div class="meta">
        <span class="pri-dot" :class="priClass(todo.priority)" :title="`优先级${todo.priority}`"></span>
        <span v-if="todo.dueTime" class="mono">{{ todo.dueTime }}</span>
        <span :class="{ 'due-late': todo.overdue }">
          {{ dateRangeText(todo) }}
        </span>
        <span
          v-if="todo.subNodeCount"
          class="chip chip-sub"
          :title="$t('子任务记录 · {a}', { a: todo.subNodeCount })"
          @click.stop="toggleSub"
        >{{ subExpanded ? '▾' : '▸' }} 📝 {{ todo.subNodeCount }}</span>
        <span
          v-if="todo.recurType"
          class="chip recur"
          :title="recurTip(todo)"
        >🔁 {{ $t(recurLabel(todo.recurType)) }}{{ todo.recurInterval > 1 ? '×' + todo.recurInterval : '' }}{{ todo.recurUntil ? ' → ' + todo.recurUntil : '' }}</span>
        <span v-if="isPinned()" class="badge pin" :title="$t('已置顶')">★ {{ $t('置顶') }}</span>
        <span v-if="todo.status === '已逾期'" class="badge danger">{{ $t('逾期') }}</span>
        <span v-for="t in todo.tags" :key="t" class="chip" :class="tagClass(t)">{{ t }}</span>
      </div>
    </div>
    <!-- 子记录目录树：缩进 + 竖线，OKR 结构（记录 = 待办的 KR/进展） -->
    <div v-if="subExpanded" class="subtree" @click.stop>
      <div v-if="subLoading" class="small muted sub-row">{{ $t('加载中…') }}</div>
      <div v-else-if="!subNodes.length" class="small muted sub-row">{{ $t('没有子记录') }}</div>
      <div v-for="n in subNodes" :key="n.id" class="sub-row small">
        <span class="sub-rail"></span>
        <span class="sub-content" :title="n.content" @click="jumpSub(n)">
          <span class="mono muted">{{ n.date.slice(5) }}</span>
          {{ n.content.slice(0, 60) }}{{ n.content.length > 60 ? '…' : '' }}
        </span>
        <span class="link muted sub-unlink" :title="$t('解除关联')" @click="unlinkSub(n.id)">✕</span>
      </div>
    </div>
    <!-- .stop：删除/建议按钮的点击不能冒泡到外层（待办页在行上绑了「点击=编辑」，否则点删除会弹出编辑框） -->
    <div v-if="showActions !== false" class="actions" @click.stop>
      <button :class="{ 'pin-on': isPinned() }" :title="isPinned() ? $t('取消置顶') : $t('置顶')" @click="togglePin">★</button>
      <button v-if="todo.status !== '已完成'" :title="$t('智伴排期建议')" @click="suggest">✨</button>
      <template v-if="askSeries">
        <button class="danger" :title="$t('已生成的后续一期也会一并删除')" @click="doRemove('series')">{{ $t('删整个循环') }}</button>
        <button @click="doRemove()">{{ $t('仅此一条') }}</button>
      </template>
      <button v-else class="danger" :class="{ 'confirm-del': confirmDelete }" @click="onRequestDelete">
        {{ confirmDelete ? $t('确认删除？') : $t('删除') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.subtree {
  flex-basis: 100%;
  margin: 2px 0 4px 30px;
  padding: 4px 0 2px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.sub-row {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  min-width: 0;
}
.sub-rail {
  width: 1px;
  align-self: stretch;
  background: var(--border, #e5e7eb);
  flex: none;
}
.sub-content {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}
.sub-content:hover { color: var(--primary, #6366f1); }
.sub-unlink { flex: none; cursor: pointer; }
.chip-sub {
  cursor: pointer;
  background: var(--primary-soft, #eef2ff);
  color: var(--primary, #6366f1);
  border: 1px solid transparent;
}
.chip-sub:hover { border-color: var(--primary, #6366f1); }
</style>
