<script setup lang="ts">
// 记录节点气泡（时间线一项）：时刻 + 内容 + 标签 + 悬停操作
import { ref } from 'vue'
import type { Node } from '@/api/types'
import { useNodesStore } from '@/stores/nodes'
import { useAppStore } from '@/stores/app'
import { useTagsStore } from '@/stores/tags'
import { t } from '@/i18n'

const props = defineProps<{ node: Node; showLine?: boolean }>()
const nodes = useNodesStore()
const app = useAppStore()
const tagsStore = useTagsStore()

const editing = ref(false)
const draft = ref('')
const draftTags = ref<string[]>([])

function timeOf(s: string) {
  return s.slice(11, 16)
}

function startEdit() {
  editing.value = true
  draft.value = props.node.content
  draftTags.value = [...props.node.tags]
  if (!tagsStore.loaded) tagsStore.load()
}

async function save() {
  if (!draft.value.trim()) return
  try {
    await nodes.update(props.node.id, draft.value.trim(), draftTags.value)
    editing.value = false
  } catch (e: any) {
    app.toast('error', e?.message || '保存失败')
  }
}

async function remove() {
  try {
    await nodes.remove(props.node.id)
    app.toast('info', t('已删除'))
    app.refreshStats()
  } catch (e: any) {
    app.toast('error', e?.message || '删除失败')
  }
}

function toggleTag(t: string) {
  const i = draftTags.value.indexOf(t)
  if (i >= 0) draftTags.value.splice(i, 1)
  else draftTags.value.push(t)
}

function tagClass(t: string) {
  const map: Record<string, string> = { 工作: 'work', 生活: 'life', 健康: 'health', 学习: 'study' }
  return map[t] || 'none'
}
</script>

<template>
  <div class="tl-item" :class="{ backfill: node.isBackfill }">
    <div class="tl-time mono">{{ timeOf(node.createdAt) }}</div>
    <div class="tl-rail">
      <div class="tl-dot"></div>
      <div v-if="showLine !== false" class="tl-line"></div>
    </div>
    <div class="tl-bubble">
      <template v-if="!editing">
        <span class="tl-actions">
          <button @click="startEdit">{{ $t('编辑') }}</button>
          <button class="danger" @click="remove">{{ $t('删除') }}</button>
        </span>
        <div v-if="node.tags.length || node.isBackfill" class="tl-meta">
          <span v-for="t in node.tags" :key="t" class="chip" :class="tagClass(t)">{{ t }}</span>
          <span v-if="node.isBackfill" class="small muted">{{ $t('补录') }}</span>
        </div>
        <div class="tl-content">{{ node.content }}</div>
      </template>
      <div v-else class="tl-edit">
        <textarea v-model="draft" class="textarea" rows="3" @keydown.esc="editing = false"></textarea>
        <div class="row wrap">
          <button
            v-for="t in tagsStore.options"
            :key="t"
            class="tag-pick"
            :class="{ on: draftTags.includes(t) }"
            @click="toggleTag(t)"
          >
            {{ t }}
          </button>
          <div class="spacer"></div>
          <button class="btn btn-sm" @click="editing = false">{{ $t('取消') }}</button>
          <button class="btn btn-sm btn-primary" @click="save">{{ $t('保存') }}</button>
        </div>
      </div>
    </div>
  </div>
</template>
