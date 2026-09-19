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
const todos = useTodosStore()
const app = useAppStore()

function tagClass(t: string) {
  const map: Record<string, string> = { 工作: 'work', 生活: 'life', 健康: 'health', 学习: 'study' }
  return map[t] || 'none'
}

function priClass(p: string) {
  return p === '高' ? 'pri-h' : p === '中' ? 'pri-m' : 'pri-l'
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
    <div class="check" :class="{ done: todo.status === '已完成' }" @click="toggle">
      <span v-if="todo.status === '已完成'">✓</span>
    </div>
    <div class="body">
      <div class="title">{{ todo.title }}</div>
      <div v-if="todo.description" class="desc" :title="todo.description">{{ todo.description }}</div>
      <div class="meta">
        <span class="pri-dot" :class="priClass(todo.priority)" :title="`优先级${todo.priority}`"></span>
        <span v-if="todo.dueTime" class="mono">{{ todo.dueTime }}</span>
        <span :class="{ 'due-late': todo.overdue }">
          {{ todo.category === '日程' && todo.dueTime ? friendlyDate(todo.dueDate) : friendlyDate(todo.dueDate) }}
        </span>
        <span
          v-if="todo.recurType"
          class="chip recur"
          :title="recurTip(todo)"
        >🔁 {{ $t(recurLabel(todo.recurType)) }}{{ todo.recurInterval > 1 ? '×' + todo.recurInterval : '' }}{{ todo.recurUntil ? ' → ' + todo.recurUntil : '' }}</span>
        <span v-if="todo.status === '已逾期'" class="badge danger">{{ $t('逾期') }}</span>
        <span v-for="t in todo.tags" :key="t" class="chip" :class="tagClass(t)">{{ t }}</span>
      </div>
    </div>
    <div v-if="showActions !== false" class="actions">
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
