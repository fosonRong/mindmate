// 待办 store：列表、筛选、四类归类、完成/改期、日程数据
import { defineStore } from 'pinia'
import { api } from '@/api/client'
import type { AppEvent, ScheduleData, Todo } from '@/api/types'
import { todayStr } from './app'

export interface TodoFilter {
  category: string
  status: string
  priority: string
  tag: string
  q: string
}

export const useTodosStore = defineStore('todos', {
  state: () => ({
    todos: [] as Todo[],
    loading: false,
    filter: { category: '全部', status: '全部', priority: '全部', tag: '', q: '' } as TodoFilter,
    /** 选中日期（待办中心右侧） */
    selectedDate: todayStr(),
    schedule: null as ScheduleData | null
  }),

  getters: {
    byCategory: (s) => (cat: string) =>
      cat === '全部' ? s.todos : s.todos.filter((t) => t.category === cat),
    /** 今日待办（含逾期的今日项） */
    todayTodos: (s) => s.todos.filter((t) => t.category === '今日'),
    weekTodos: (s) => s.todos.filter((t) => t.category === '本周'),
    monthTodos: (s) => s.todos.filter((t) => t.category === '本月'),
    scheduleTodos: (s) => s.todos.filter((t) => t.category === '日程'),
    overdue: (s) => s.todos.filter((t) => t.status === '已逾期'),
    pendingCount: (s) => s.todos.filter((t) => t.status !== '已完成').length,
    progress: (s) => {
      const total = s.todos.length
      const done = s.todos.filter((t) => t.status === '已完成').length
      return { total, done, percent: total ? Math.round((done / total) * 100) : 0 }
    }
  },

  actions: {
    async load() {
      this.loading = true
      try {
        const params: Record<string, string> = {}
        if (this.filter.category !== '全部') params.category = this.filter.category
        if (this.filter.status !== '全部') params.status = this.filter.status
        if (this.filter.priority !== '全部') params.priority = this.filter.priority
        if (this.filter.tag) params.tag = this.filter.tag
        if (this.filter.q) params.q = this.filter.q
        this.todos = await api.todos(params)
      } finally {
        this.loading = false
      }
    },

    async create(data: {
      title: string
      description?: string
      dueDate: string
      dueTime?: string | null
      priority?: string
      tags?: string[]
      remindOffsetMin?: number
      /** 循环类型：''=不循环 daily=每天 weekly=每周 monthly=每月 */
      recurType?: string
    }) {
      const todo = await api.createTodo(data as any)
      // 幂等：SSE 事件可能先于 HTTP 响应到达并已插入同一条待办
      const exist = this.todos.findIndex((t) => t.id === todo.id)
      if (exist >= 0) this.todos[exist] = todo
      else this.todos.unshift(todo)
      return todo
    },

    async update(id: number, data: Record<string, unknown>) {
      const todo = await api.updateTodo(id, data)
      const i = this.todos.findIndex((t) => t.id === id)
      if (i >= 0) this.todos[i] = todo
      return todo
    },

    async toggle(id: number) {
      const t = this.todos.find((x) => x.id === id)
      if (!t) return
      const updated = t.status === '已完成' ? await api.reopenTodo(id) : await api.completeTodo(id)
      const i = this.todos.findIndex((x) => x.id === id)
      if (i >= 0) this.todos[i] = updated
    },

    async remove(id: number) {
      await api.deleteTodo(id)
      const i = this.todos.findIndex((t) => t.id === id)
      if (i >= 0) this.todos.splice(i, 1)
    },

    /** 改期（拖拽/按钮） */
    async reschedule(id: number, dueDate: string) {
      await this.update(id, { dueDate })
    },

    async loadSchedule(date?: string) {
      if (date) this.selectedDate = date
      this.schedule = await api.schedule(this.selectedDate)
      return this.schedule
    },

    onEvent(ev: AppEvent) {
      const id = ev.payload?.id
      if (!id) return
      if (ev.kind === 'todo.created') {
        if (!this.todos.some((t) => t.id === id)) this.todos.unshift(ev.payload)
      } else if (ev.kind === 'todo.updated' || ev.kind === 'todo.completed') {
        const i = this.todos.findIndex((t) => t.id === id)
        if (i >= 0) this.todos[i] = ev.payload
        else this.todos.unshift(ev.payload)
      } else if (ev.kind === 'todo.deleted') {
        const i = this.todos.findIndex((t) => t.id === id)
        if (i >= 0) this.todos.splice(i, 1)
      }
    }
  }
})
