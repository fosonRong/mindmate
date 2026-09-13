<script setup lang="ts">
// 速记输入框：页面常驻入口 —— 一次提交 = 一个节点
import { ref, onMounted, onUnmounted, nextTick } from 'vue'
import { useNodesStore } from '@/stores/nodes'
import { useAppStore } from '@/stores/app'

const props = defineProps<{ date?: string; autofocus?: boolean; compact?: boolean }>()
const emit = defineEmits<{ (e: 'saved'): void }>()

const nodes = useNodesStore()
const app = useAppStore()
const content = ref('')
const pickedTags = ref<string[]>([])
const TAG_OPTIONS = ['工作', '生活', '健康', '学习']
const inputEl = ref<HTMLInputElement | null>(null)
const saving = ref(false)

function toggleTag(t: string) {
  const i = pickedTags.value.indexOf(t)
  if (i >= 0) pickedTags.value.splice(i, 1)
  else pickedTags.value.push(t)
}

async function submit() {
  if (!content.value.trim() || saving.value) return
  saving.value = true
  try {
    await nodes.create(content.value, [...pickedTags.value], props.date)
    content.value = ''
    app.toast('success', `已记录 ${new Date().toTimeString().slice(0, 5)}`)
    app.refreshStats()
    emit('saved')
    await nextTick()
    inputEl.value?.focus()
  } catch (e: any) {
    app.toast('error', e?.message || '记录失败')
  } finally {
    saving.value = false
  }
}

function focus() {
  inputEl.value?.focus()
}

function onGlobalFocus() {
  focus()
}

onMounted(() => {
  if (props.autofocus) nextTick(() => inputEl.value?.focus())
  window.addEventListener('mindmate:focus-quick-entry', onGlobalFocus)
})
onUnmounted(() => window.removeEventListener('mindmate:focus-quick-entry', onGlobalFocus))

defineExpose({ focus })
</script>

<template>
  <div class="quick-entry">
    <input
      ref="inputEl"
      v-model="content"
      type="text"
      placeholder="快速记录此刻的工作 / 生活…"
      @keydown.enter.prevent="submit"
    />
    <div class="tags-row" :style="compact ? 'opacity:1' : ''">
      <button
        v-for="t in TAG_OPTIONS"
        :key="t"
        class="tag-pick"
        :class="{ on: pickedTags.includes(t) }"
        @click="toggleTag(t)"
      >
        {{ t }}
      </button>
      <span class="hotkey">Enter 保存</span>
    </div>
  </div>
</template>
