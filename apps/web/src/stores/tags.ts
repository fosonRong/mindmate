/**
 * 标签中心（v1.1.1）：速记 / 待办弹窗 / 节点编辑共用的标签选项。
 *
 * 选项 = 内置四枚 ∪ 用户自定义（settings.custom_tags）∪ 已在用（/api/v1/tags 统计）。
 * 此前各组件各写一份 ['工作','生活','健康','学习']，用户没法加自己的标签；
 * 统一收敛到这里后，设置页加删自定义词，所有输入处即时生效。
 */
import { defineStore } from 'pinia'
import { api } from '@/api/client'
import type { TagStat } from '@/api/types'
import { useAppStore } from './app'

export const DEFAULT_TAGS = ['工作', '生活', '健康', '学习']

export const useTagsStore = defineStore('tags', {
  state: () => ({
    /** 记录 + 待办里真实用过的标签（带使用次数，降序） */
    used: [] as TagStat[],
    loaded: false
  }),

  getters: {
    /** 用户自定义标签（settings.custom_tags JSON 数组） */
    custom(state): string[] {
      const app = useAppStore()
      try {
        const arr = JSON.parse(app.settings.custom_tags || '[]')
        return Array.isArray(arr) ? arr.filter((t: unknown) => typeof t === 'string' && t.trim()) : []
      } catch {
        return []
      }
    },
    /** 标签选择器完整选项：内置 → 自定义 → 在用，去重保序 */
    options(state): string[] {
      const out: string[] = []
      for (const t of [...DEFAULT_TAGS, ...this.custom, ...state.used.map((u) => u.name)]) {
        if (t && !out.includes(t)) out.push(t)
      }
      return out
    },
    usageMap(state): Record<string, number> {
      const m: Record<string, number> = {}
      state.used.forEach((u) => (m[u.name] = u.count))
      return m
    }
  },

  actions: {
    async load() {
      try {
        this.used = await api.listTags()
        this.loaded = true
      } catch {
        /* 标签统计非核心路径，失败保持空 */
      }
    },
    /** 新增自定义标签（去重、限 12 字），写回 settings.custom_tags */
    async addCustom(name: string) {
      const t = name.trim().replace(/^#/, '').trim()
      if (!t) return
      if (t.length > 12) throw new Error('标签不超过 12 个字')
      if (DEFAULT_TAGS.includes(t) || this.custom.includes(t)) return
      const next = [...this.custom, t]
      const app = useAppStore()
      await app.saveSettings({ custom_tags: JSON.stringify(next) })
    },
    /** 删除自定义标签（内置四枚不可删；在用统计不受影响） */
    async removeCustom(name: string) {
      if (DEFAULT_TAGS.includes(name)) return
      const next = this.custom.filter((t) => t !== name)
      const app = useAppStore()
      await app.saveSettings({ custom_tags: JSON.stringify(next) })
    }
  }
})
