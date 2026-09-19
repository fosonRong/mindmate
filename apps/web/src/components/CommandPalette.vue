<script setup lang="ts">
// 全局命令面板（v1.1.3）：Ctrl+K 呼出 —— 统一搜索（记录/待办/报告）+ 页面跳转 + 新建动作。
// 键盘优先：↑↓ 选择、Enter 执行、Esc 关闭；空查询时显示快捷入口。
import { computed, onMounted, onUnmounted, ref, watch, nextTick } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '@/api/client'
import type { SearchResults } from '@/api/types'
import { todayStr, friendlyDate } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { t } from '@/i18n'

const router = useRouter()
const nodes = useNodesStore()

const open = ref(false)
const query = ref('')
const searching = ref(false)
const results = ref<SearchResults | null>(null)
const active = ref(0)
const inputEl = ref<HTMLInputElement | null>(null)
let debounceTimer: ReturnType<typeof setTimeout> | null = null
let searchSeq = 0

/** 静态命令：跳转 + 动作（空查询 / 搜索结果之外始终可用） */
interface Cmd {
  id: string
  icon: string
  label: string
  hint?: string
  run: () => void
}

const commands: Cmd[] = [
  { id: 'nav-today', icon: '🏠', label: t('去今日'), hint: '今日 · 速记 · 我的简报', run: () => go('#/') },
  { id: 'nav-week', icon: '📅', label: t('去周视图'), hint: t('本周一览'), run: () => go('#/week') },
  { id: 'nav-month', icon: '🗓️', label: t('去月视图'), hint: t('日历 · 热力 · 补录'), run: () => go('#/month') },
  { id: 'nav-todos', icon: '✅', label: t('去待办'), hint: t('全部待办管理'), run: () => go('#/todos') },
  { id: 'nav-companion', icon: '🤖', label: t('去智伴'), hint: t('报告 · 复盘 · 问答'), run: () => go('#/companion') },
  { id: 'nav-settings', icon: '⚙️', label: t('去设置'), hint: t('提醒 · AI · 标签 · 数据'), run: () => go('#/settings') },
  {
    id: 'act-new-node',
    icon: '⚡',
    label: t('新建记录'),
    hint: t('到今日页速记框输入'),
    run: () => {
      go('#/')
      setTimeout(() => window.dispatchEvent(new CustomEvent('mindmate:focus-quick-entry')), 350)
    }
  },
  {
    id: 'act-new-todo',
    icon: '➕',
    label: t('新建待办'),
    hint: t('打开待办新建弹窗'),
    run: () => {
      go('#/todos')
      setTimeout(() => window.dispatchEvent(new CustomEvent('mindmate:new-todo')), 350)
    }
  }
]

function go(hash: string) {
  if (location.hash === hash) return
  location.hash = hash
}

interface Row {
  key: string
  icon: string
  label: string
  hint: string
  run: () => void
}

const commandRows = computed<Row[]>(() =>
  commands.map((c) => ({ key: c.id, icon: c.icon, label: c.label, hint: c.hint || '', run: c.run }))
)

const nodeRows = computed<Row[]>(() =>
  (results.value?.nodes || []).map((n) => ({
    key: `node-${n.id}`,
    icon: '📝',
    label: n.content.length > 60 ? `${n.content.slice(0, 60)}…` : n.content,
    hint: n.date === todayStr() ? t('今日') : friendlyDate(n.date),
    run: () => {
      go('#/month')
      // 月视图监听：切到该日所在月份并打开日详情抽屉
      setTimeout(() => window.dispatchEvent(new CustomEvent('mindmate:open-day', { detail: { date: n.date } })), 350)
    }
  }))
)

const todoRows = computed<Row[]>(() =>
  (results.value?.todos || []).map((x) => ({
    key: `todo-${x.id}`,
    icon: '✅',
    label: x.title,
    hint: `${x.dueDate}${x.status ? ' · ' + x.status : ''}`,
    run: () => go('#/todos')
  }))
)

const reportRows = computed<Row[]>(() =>
  (results.value?.reports || []).map((r) => ({
    key: `report-${r.id}`,
    icon: '📄',
    label: r.snippet,
    hint: `${r.type === 'daily' ? t('日报') : r.type === 'weekly' ? t('周报') : r.type === 'monthly' ? t('月报') : r.type} · ${r.period}`,
    run: () => go('#/companion')
  }))
)

const allRows = computed<Row[]>(() => {
  const hasQuery = query.value.trim().length > 0
  const base = hasQuery ? [] : commandRows.value
  return [...base, ...nodeRows.value, ...todoRows.value, ...reportRows.value]
})

function onKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && (e.key === 'k' || e.key === 'K')) {
    e.preventDefault()
    toggle()
    return
  }
  if (!open.value) return
  if (e.key === 'Escape') {
    e.preventDefault()
    close()
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    if (allRows.value.length) active.value = (active.value + 1) % allRows.value.length
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    if (allRows.value.length) active.value = (active.value - 1 + allRows.value.length) % allRows.value.length
  } else if (e.key === 'Enter') {
    e.preventDefault()
    const row = allRows.value[active.value]
    if (row) execute(row)
  }
}

function toggle() {
  if (open.value) close()
  else show()
}

function show() {
  open.value = true
  query.value = ''
  results.value = null
  active.value = 0
  nextTick(() => inputEl.value?.focus())
}

function close() {
  open.value = false
}

function execute(row: Row) {
  close()
  row.run()
}

function onInput() {
  if (debounceTimer) clearTimeout(debounceTimer)
  const q = query.value.trim()
  if (!q) {
    results.value = null
    active.value = 0
    return
  }
  debounceTimer = setTimeout(async () => {
    const seq = ++searchSeq
    searching.value = true
    try {
      const r = await api.search({ q, limit: 10 })
      if (seq !== searchSeq) return
      results.value = r
      active.value = 0
    } catch {
      if (seq === searchSeq) results.value = null
    } finally {
      if (seq === searchSeq) searching.value = false
    }
  }, 250)
}

/** 分组渲染时把局部下标对回 allRows 全局下标 */
function isActive(globalIndex: number) {
  return active.value === globalIndex
}

watch(active, () => {
  nextTick(() => {
    document.querySelector('.palette-row.active')?.scrollIntoView({ block: 'nearest' })
  })
})

watch(query, onInput)
// 搜索结果变化后回到第一项
watch(allRows, () => {
  if (active.value >= allRows.value.length) active.value = 0
})

// 全局事件：顶栏搜索按钮 / 其他组件呼出
function onGlobalOpen() {
  show()
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('mindmate:open-palette', onGlobalOpen)
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('mindmate:open-palette', onGlobalOpen)
  if (debounceTimer) clearTimeout(debounceTimer)
})
</script>

<template>
  <teleport to="body">
    <div v-if="open" class="palette-mask" @mousedown.self="close">
      <div class="palette">
        <div class="palette-input-row">
          <span class="palette-icon">🔍</span>
          <input
            ref="inputEl"
            v-model="query"
            class="palette-input"
            type="text"
            :placeholder="$t('搜索记录、待办、报告，或输入命令…')"
            @keydown.enter.prevent
          />
          <span class="palette-esc mono">Esc</span>
        </div>

        <div class="palette-list">
          <!-- 空查询：快捷命令 -->
          <template v-if="!query.trim()">
            <div class="palette-group">{{ $t('快捷操作') }}</div>
            <button
              v-for="(row, i) in commandRows"
              :key="row.key"
              class="palette-row"
              :class="{ active: i === active }"
              @click="execute(row)"
              @mousemove="active = i"
            >
              <span class="palette-row-icon">{{ row.icon }}</span>
              <span class="palette-row-label">{{ row.label }}</span>
              <span class="palette-row-hint">{{ row.hint }}</span>
            </button>
          </template>

          <!-- 搜索结果：分组展示 -->
          <template v-else>
            <template v-if="nodeRows.length">
              <div class="palette-group">{{ $t('记录（{a}）', { a: nodeRows.length }) }}</div>
              <button
                v-for="(row, i) in nodeRows"
                :key="row.key"
                class="palette-row"
                :class="{ active: isActive(i) }"
                @click="execute(row)"
                @mousemove="active = i"
              >
                <span class="palette-row-icon">{{ row.icon }}</span>
                <span class="palette-row-label">{{ row.label }}</span>
                <span class="palette-row-hint">{{ row.hint }}</span>
              </button>
            </template>
            <template v-if="todoRows.length">
              <div class="palette-group">{{ $t('待办（{a}）', { a: todoRows.length }) }}</div>
              <button
                v-for="(row, i) in todoRows"
                :key="row.key"
                class="palette-row"
                :class="{ active: isActive(nodeRows.length + i) }"
                @click="execute(row)"
                @mousemove="active = nodeRows.length + i"
              >
                <span class="palette-row-icon">{{ row.icon }}</span>
                <span class="palette-row-label">{{ row.label }}</span>
                <span class="palette-row-hint">{{ row.hint }}</span>
              </button>
            </template>
            <template v-if="reportRows.length">
              <div class="palette-group">{{ $t('报告（{a}）', { a: reportRows.length }) }}</div>
              <button
                v-for="(row, i) in reportRows"
                :key="row.key"
                class="palette-row"
                :class="{ active: isActive(nodeRows.length + todoRows.length + i) }"
                @click="execute(row)"
                @mousemove="active = nodeRows.length + todoRows.length + i"
              >
                <span class="palette-row-icon">{{ row.icon }}</span>
                <span class="palette-row-label">{{ row.label }}</span>
                <span class="palette-row-hint">{{ row.hint }}</span>
              </button>
            </template>
            <div v-if="!allRows.length && !searching" class="palette-empty">
              {{ $t('没有找到与「{a}」相关的内容', { a: query.trim() }) }}
            </div>
            <div v-if="searching" class="palette-empty">{{ $t('搜索中…') }}</div>
            <!-- 有查询时仍保留命令入口 -->
            <div v-if="allRows.length" class="palette-group">{{ $t('命令') }}</div>
            <button
              v-for="(row, i) in commandRows"
              :key="row.key"
              class="palette-row"
              :class="{ active: isActive(allRows.length + i) }"
              @click="execute(row)"
              @mousemove="active = allRows.length + i"
            >
              <span class="palette-row-icon">{{ row.icon }}</span>
              <span class="palette-row-label">{{ row.label }}</span>
              <span class="palette-row-hint">{{ row.hint }}</span>
            </button>
          </template>
        </div>
        <div class="palette-foot small muted">
          <span class="mono">↑↓</span> {{ $t('选择') }} · <span class="mono">Enter</span> {{ $t('打开') }} ·
          <span class="mono">Esc</span> {{ $t('关闭') }}
        </div>
      </div>
    </div>
  </teleport>
</template>

