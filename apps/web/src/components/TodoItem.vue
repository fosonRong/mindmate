<script setup lang="ts">
// 待办项：复选框、优先级、逾期徽标、悬停操作、拖拽
import type { Todo } from '@/api/types'
import { useTodosStore } from '@/stores/todos'
import { useAppStore, friendlyDate } from '@/stores/app'

const props = defineProps<{ todo: Todo; draggable?: boolean; showActions?: boolean }>()
const todos = useTodosStore()
const app = useAppStore()

function tagClass(t: string) {
  const map: Record<string, string> = { 工作: 'work', 生活: 'life', 健康: 'health', 学习: 'study' }
  return map[t] || 'none'
}

function priClass(p: string) {
  return p === '高' ? 'pri-h' : p === '中' ? 'pri-m' : 'pri-l'
}

async function toggle() {
  try {
    await todos.toggle(props.todo.id)
    app.refreshStats()
  } catch (e: any) {
    app.toast('error', e?.message || '操作失败')
  }
}

async function remove() {
  try {
    await todos.remove(props.todo.id)
    app.toast('info', '已删除待办')
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
        <span v-if="todo.status === '已逾期'" class="badge danger">{{ $t('逾期') }}</span>
        <span v-for="t in todo.tags" :key="t" class="chip" :class="tagClass(t)">{{ t }}</span>
      </div>
    </div>
    <div v-if="showActions !== false" class="actions">
      <button v-if="todo.status !== '已完成'" :title="$t('智伴排期建议')" @click="suggest">✨</button>
      <button class="danger" @click="remove">{{ $t('删除') }}</button>
    </div>
  </div>
</template>
