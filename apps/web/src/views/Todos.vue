<script setup lang="ts">
// 待办中心：左侧分类列表 + 右侧日历与当日【具体日程 / 待办事项】双栏
import { computed, onMounted, ref, watch } from 'vue'
import { useTodosStore } from '@/stores/todos'
import { useAppStore, todayStr, friendlyDate, fmtDate } from '@/stores/app'
import TodoItem from '@/components/TodoItem.vue'
import TodoEditModal from '@/components/TodoEditModal.vue'
import CalendarMonth from '@/components/CalendarMonth.vue'
import { api } from '@/api/client'
import type { PeriodStats, Todo } from '@/api/types'
import { t } from '@/i18n'

const todos = useTodosStore()
const app = useAppStore()

const tab = ref<'全部' | '今日' | '本周' | '本月'>('全部')
const showModal = ref(false)
const editing = ref<Todo | null>(null)
const periodStats = ref<PeriodStats | null>(null)
const calAnchor = ref(todayStr())

const today = todayStr()

/** 按分类分组展示 */
const grouped = computed(() => {
  const cats: Array<'今日' | '本周' | '本月' | '日程'> = ['今日', '本周', '本月', '日程']
  return cats
    .map((c) => ({
      category: c,
      list: todos.todos.filter((t) => t.category === c)
    }))
    .filter((g) => g.list.length > 0)
})

const progress = computed(() => {
  const list = tab.value === '全部' ? todos.todos : todos.todos.filter((t) => t.category === tab.value)
  const total = list.length
  const done = list.filter((t) => t.status === '已完成').length
  return { total, done, percent: total ? Math.round((done / total) * 100) : 0 }
})

/** 日历上的待办数量分布 */
const todoCounts = computed(() => {
  const map: Record<string, number> = {}
  todos.todos.forEach((t) => {
    if (t.status === '已完成') return
    map[t.dueDate] = (map[t.dueDate] || 0) + 1
  })
  return map
})

/** 每日待办标题（用于日历格内展示标题，而非仅圆点） */
const todoTitles = computed(() => {
  const map: Record<string, string[]> = {}
  todos.todos
    .filter((t) => t.status !== '已完成')
    .forEach((t) => {
      if (!map[t.dueDate]) map[t.dueDate] = []
      map[t.dueDate].push(t.dueTime ? `${t.dueTime} ${t.title}` : t.title)
    })
  return map
})

async function loadAll() {
  await Promise.all([todos.load(), todos.loadSchedule(todos.selectedDate)])
  const from = `${todos.selectedDate.slice(0, 8)}01`
  const end = new Date(todos.selectedDate.replace(/-/g, '/'))
  end.setMonth(end.getMonth() + 1)
  end.setDate(0)
  const to = `${end.getFullYear()}-${String(end.getMonth() + 1).padStart(2, '0')}-${String(end.getDate()).padStart(2, '0')}`
  try {
    periodStats.value = await api.periodStats(from, to)
  } catch {
    /* 忽略 */
  }
}

watch(tab, async () => {
  todos.filter.category = tab.value
  await todos.load()
})

function shiftMonth(n: number) {
  const d = new Date(calAnchor.value.replace(/-/g, '/'))
  d.setMonth(d.getMonth() + n)
  calAnchor.value = fmtDate(d)
  loadAll()
}

/** 月份筛选（input[type=month] → YYYY-MM） */
function pickMonth(e: Event) {
  const v = (e.target as HTMLInputElement).value
  if (!v) return
  calAnchor.value = `${v}-01`
  loadAll()
}

async function selectDate(date: string) {
  todos.selectedDate = date
  await todos.loadSchedule(date)
}

async function onDropTodo(payload: { id: number; date: string }) {
  await todos.reschedule(payload.id, payload.date)
  app.toast('success', t('已改期至 {a}', { a: payload.date }))
  await loadAll()
}

function newTodo() {
  editing.value = null
  showModal.value = true
}

function editTodo(t: Todo) {
  editing.value = t
  showModal.value = true
}

async function onSaved() {
  showModal.value = false
  await loadAll()
}

onMounted(loadAll)
</script>

<template>
  <div class="col-stack">
    <!-- 筛选条 -->
    <section class="card">
      <div class="row wrap">
        <div class="seg">
          <button v-for="c in (['全部', '今日', '本周', '本月'] as const)" :key="c" :class="{ on: tab === c }" @click="tab = c">
            {{ c }}
          </button>
        </div>
        <div class="select-wrap">
          <select v-model="todos.filter.status" class="input" style="height: 32px; width: 118px; font-size: 12px" @change="todos.load()">
            <option v-for="s in ['全部', '待处理', '进行中', '已逾期', '已完成']" :key="s" :value="s">{{ s === '全部' ? $t('全部状态') : s }}</option>
          </select>
        </div>
        <div class="select-wrap">
          <select v-model="todos.filter.priority" class="input" style="height: 32px; width: 110px; font-size: 12px" @change="todos.load()">
            <option v-for="p in ['全部', '高', '中', '低']" :key="p" :value="p">{{ p === '全部' ? $t('全部优先级') : p }}</option>
          </select>
        </div>
        <input
          v-model="todos.filter.q"
          class="input"
          style="height: 32px; flex: 1; min-width: 120px; font-size: 12px"
          :placeholder="$t('搜索待办…')"
          @keydown.enter="todos.load()"
        />
        <button class="btn btn-sm" @click="loadAll">{{ $t('刷新') }}</button>
        <button class="btn btn-sm btn-primary" @click="newTodo">{{ $t('＋ 新建待办') }}</button>
      </div>
      <div class="row" style="margin-top: 10px">
        <div class="progress" :class="progress.percent > 70 ? 'p-high' : progress.percent >= 30 ? 'p-mid' : 'p-low'" style="flex: 1">
          <i :style="{ width: progress.percent + '%' }"></i>
        </div>
        <span class="small muted" style="width: 96px; text-align: right">
          {{ tab }} {{ progress.done }}/{{ progress.total }}
        </span>
      </div>
    </section>

    <div class="todos-split">
      <!-- 左：待办列表（按分类分组） -->
      <div class="col-stack">
        <section v-if="todos.todos.length === 0" class="card">
          <div class="empty">
            <div class="ill">✨</div>
            <div class="t">{{ $t('此刻一身轻') }}</div>
            <div class="d">{{ $t('没有待办，或添加一件') }}</div>
            <button class="btn btn-primary" style="margin-top: 6px" @click="newTodo">{{ $t('＋ 添加待办') }}</button>
          </div>
        </section>

        <section v-for="g in grouped" :key="g.category" class="card">
          <div class="row" style="margin-bottom: 6px">
            <div class="card-title" style="font-size: 14px">{{ $t('{a}待办', { a: g.category }) }}</div>
            <div class="spacer"></div>
            <span class="small muted">{{ $t('{a}/{b}', { a: g.list.filter((t) => t.status === '已完成').length, b: g.list.length }) }}</span>
          </div>
          <div class="progress" style="margin-bottom: 6px">
            <i
              :style="{
                width: (g.list.length ? Math.round((g.list.filter((t) => t.status === '已完成').length / g.list.length) * 100) : 0) + '%'
              }"
            ></i>
          </div>
          <TodoItem
            v-for="t in g.list"
            :key="t.id"
            :todo="t"
            draggable
            @click="editTodo(t)"
          />
        </section>
      </div>

      <!-- 右：日历 + 当日双栏 -->
      <div class="col-stack">
        <section class="card">
          <div class="row wrap" style="margin-bottom: 10px; gap: 6px">
            <button class="icon-btn" :title="$t('上个月')" @click="shiftMonth(-1)">◀</button>
            <b style="font-size: 14px">{{ calAnchor.slice(0, 7) }}</b>
            <button class="icon-btn" :title="$t('下个月')" @click="shiftMonth(1)">▶</button>
            <input
              type="month"
              class="input"
              style="width: 140px; height: 30px; font-size: 12px"
              :value="calAnchor.slice(0, 7)"
              @change="pickMonth"
            />
            <button class="btn btn-sm" @click="calAnchor = todayStr(); loadAll()">{{ $t('本月') }}</button>
            <div class="spacer"></div>
          </div>
          <CalendarMonth
            :month-date="calAnchor"
            :days="periodStats?.days || []"
            :selected="todos.selectedDate"
            :todo-counts="todoCounts"
            :todo-titles="todoTitles"
            :max-todo-titles="2"
            compact
            @select="selectDate"
            @drop-todo="onDropTodo"
          />
          <div class="small muted" style="margin-top: 8px">{{ $t('提示：把左侧待办拖到日历上的某天即可改期') }}</div>
        </section>

        <section class="card">
          <div class="card-title" style="font-size: 14px">
            {{ $t('{a} {b} · 具体日程', { a: todos.selectedDate, b: friendlyDate(todos.selectedDate) }) }}
          </div>
          <div v-if="!todos.schedule?.schedules.length" class="small muted" style="margin-top: 6px">{{ $t('当天没有定时日程') }}</div>
          <div v-for="s in todos.schedule?.schedules || []" :key="s.id" class="row" style="margin-top: 8px; gap: 10px">
            <span class="mono small muted">{{ s.dueTime }}</span>
            <span style="font-size: 13px" :style="s.status === '已完成' ? 'text-decoration:line-through;color:var(--text-disable)' : ''">
              {{ s.title }}
            </span>
            <span v-if="s.overdue" class="badge danger">{{ $t('逾期') }}</span>
            <div class="spacer"></div>
            <button class="btn btn-sm" @click="todos.toggle(s.id)">{{ s.status === '已完成' ? $t('撤销') : $t('完成') }}</button>
            <button class="btn btn-sm" @click="editTodo(s)">{{ $t('改期') }}</button>
          </div>

          <div class="divider" style="margin: 14px 0"></div>

          <div class="card-title" style="font-size: 14px">
            {{ $t('{a} · 待办事项', { a: todos.selectedDate }) }}
          </div>
          <div v-if="!todos.schedule?.todos.length" class="small muted" style="margin-top: 6px">{{ $t('当天没有待办') }}</div>
          <TodoItem v-for="t in todos.schedule?.todos || []" :key="t.id" :todo="t" />
          <button class="btn" style="width: 100%; justify-content: center; margin-top: 8px" @click="newTodo">
            {{ $t('＋ 添加该日待办') }}
          </button>
        </section>
      </div>
    </div>

    <TodoEditModal v-if="showModal" :todo="editing" :default-date="todos.selectedDate" @close="showModal = false" @saved="onSaved" />
  </div>
</template>
