// 记录节点 store：按日加载、增删改、跨端同步
import { defineStore } from 'pinia'
import { api } from '@/api/client'
import type { AppEvent, Node } from '@/api/types'
import { todayStr } from './app'

export const useNodesStore = defineStore('nodes', {
  state: () => ({
    date: todayStr(),
    nodes: [] as Node[],
    loading: false,
    /** 周期缓存（周/月视图） */
    rangeCache: {} as Record<string, Node[]>
  }),

  getters: {
    count: (s) => s.nodes.length,
    isEmpty: (s) => s.nodes.length === 0
  },

  actions: {
    async load(date?: string) {
      if (date) this.date = date
      this.loading = true
      try {
        this.nodes = await api.nodesByDate(this.date)
      } finally {
        this.loading = false
      }
    },

    async loadRange(from: string, to: string, key?: string) {
      const cacheKey = key || `${from}~${to}`
      const nodes = await api.nodesRange(from, to)
      this.rangeCache[cacheKey] = nodes
      return nodes
    },

    /** 录入：一次提交 = 一个节点 */
    async create(content: string, tags: string[] = [], date?: string) {
      const text = content.trim()
      if (!text) return null
      const node = await api.createNode({ content: text, tags, date: date || this.date })
      if (node.date === this.date) {
        // 幂等：SSE 事件可能先于 HTTP 响应到达并已插入同一节点
        const exist = this.nodes.findIndex((n) => n.id === node.id)
        if (exist >= 0) this.nodes[exist] = node
        else this.nodes.push(node)
        this.nodes.sort((a, b) => a.createdAt.localeCompare(b.createdAt))
      }
      // 使周期缓存失效
      this.rangeCache = {}
      return node
    },

    async update(id: number, content: string, tags?: string[]) {
      const node = await api.updateNode(id, { content, tags })
      const i = this.nodes.findIndex((n) => n.id === id)
      if (i >= 0) this.nodes[i] = node
      this.rangeCache = {}
      return node
    },

    async remove(id: number) {
      await api.deleteNode(id)
      const i = this.nodes.findIndex((n) => n.id === id)
      if (i >= 0) this.nodes.splice(i, 1)
      this.rangeCache = {}
    },

    /** 响应 SSE 事件（其他端操作同步） */
    onEvent(ev: AppEvent) {
      if (ev.kind === 'node.created' && ev.payload?.date === this.date) {
        if (!this.nodes.some((n) => n.id === ev.payload.id)) {
          this.nodes.push(ev.payload)
          this.nodes.sort((a, b) => a.createdAt.localeCompare(b.createdAt))
        }
        this.rangeCache = {}
      } else if (ev.kind === 'node.updated') {
        const i = this.nodes.findIndex((n) => n.id === ev.payload?.id)
        if (i >= 0) this.nodes[i] = ev.payload
        this.rangeCache = {}
      } else if (ev.kind === 'node.deleted') {
        const i = this.nodes.findIndex((n) => n.id === ev.payload?.id)
        if (i >= 0) this.nodes.splice(i, 1)
        this.rangeCache = {}
      }
    }
  }
})
