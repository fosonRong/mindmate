<script setup lang="ts">
// 速记输入框：页面常驻入口 —— 一次提交 = 一个节点
// v1.1.1：标签选项改为「内置 ∪ 自定义 ∪ 在用」（stores/tags），支持 AI 打标：
// 内容停顿 2.5s 自动建议（设置 ai_autotag 可关），或点 ✨ 立即建议，建议直接选中可再改。
import { ref, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { useNodesStore } from '@/stores/nodes'
import { useAppStore } from '@/stores/app'
import { useTagsStore } from '@/stores/tags'
import { api } from '@/api/client'
import { t } from '@/i18n'

const props = defineProps<{ date?: string; autofocus?: boolean; compact?: boolean }>()
const emit = defineEmits<{ (e: 'saved'): void }>()

const nodes = useNodesStore()
const app = useAppStore()
const tagsStore = useTagsStore()
const content = ref('')
const pickedTags = ref<string[]>([])
const inputEl = ref<HTMLInputElement | null>(null)
const saving = ref(false)

// ── 新增自定义标签（＋ 展开一行小输入） ──
const addingTag = ref(false)
const newTag = ref('')

function toggleTag(t: string) {
  const i = pickedTags.value.indexOf(t)
  if (i >= 0) pickedTags.value.splice(i, 1)
  else pickedTags.value.push(t)
}

async function confirmNewTag() {
  const name = newTag.value.trim()
  if (!name) {
    addingTag.value = false
    return
  }
  try {
    await tagsStore.addCustom(name)
    if (!pickedTags.value.includes(name)) pickedTags.value.push(name)
    newTag.value = ''
    addingTag.value = false
  } catch (e: any) {
    app.toast('error', e?.message || t('添加失败'))
  }
}

// ── AI 打标 ──
const tagSuggesting = ref(false)
let suggestTimer: ReturnType<typeof setTimeout> | null = null
let lastSuggestedFor = '' // 该内容已建议过就不再重复打（含自动触发）
let suggestSeq = 0 // 内容已变化的过期响应直接丢弃

/** 请求 AI 建议标签并选中（manual=false 时是停顿自动触发：失败静默不打扰） */
async function suggestTags(manual: boolean) {
  const text = content.value.trim()
  if (!text || tagSuggesting.value) return
  const seq = ++suggestSeq
  lastSuggestedFor = text
  tagSuggesting.value = true
  try {
    const r = await api.aiTag(text)
    if (seq !== suggestSeq) return // 输入已变化，丢弃过期建议
    const fresh = r.tags.filter((x) => !pickedTags.value.includes(x))
    if (fresh.length) {
      pickedTags.value.push(...fresh)
      if (manual) app.toast('success', t('AI 建议标签：{a}', { a: fresh.join('、') }))
    } else if (manual) {
      app.toast('info', t('AI 没有想到更合适的标签'))
    }
  } catch (e: any) {
    if (manual) app.toast('error', e?.message || t('AI 打标失败，请稍后再试'))
  } finally {
    if (seq === suggestSeq) tagSuggesting.value = false
  }
}

/** 停顿自动打标：AI 就绪 + 设置开启 + 内容 ≥8 字，停 2.5s 触发一次 */
function onContentInput() {
  if (suggestTimer) { clearTimeout(suggestTimer); suggestTimer = null }
  if (app.settings.ai_autotag === '0' || !app.aiReady) return
  const text = content.value.trim()
  if (!text || text === lastSuggestedFor || text.length < 8) return
  suggestTimer = setTimeout(() => suggestTags(false), 2500)
}

watch(content, onContentInput)
// 用户关掉自动打标 / 配好 AI 后立即生效
watch(
  () => [app.settings.ai_autotag, app.aiReady],
  () => {
    if (suggestTimer) { clearTimeout(suggestTimer); suggestTimer = null }
  }
)

async function submit() {
  if (!content.value.trim() || saving.value) return
  if (suggestTimer) { clearTimeout(suggestTimer); suggestTimer = null }
  saving.value = true
  try {
    await nodes.create(content.value, [...pickedTags.value], props.date)
    content.value = ''
    lastSuggestedFor = ''
    app.toast('success', t('已记录 {a}', { a: new Date().toTimeString().slice(0, 5) }))
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
  tagsStore.load()
  window.addEventListener('mindmate:focus-quick-entry', onGlobalFocus)
})
onUnmounted(() => {
  window.removeEventListener('mindmate:focus-quick-entry', onGlobalFocus)
  if (suggestTimer) { clearTimeout(suggestTimer); suggestTimer = null }
})

defineExpose({ focus })
</script>

<template>
  <div class="quick-entry">
    <input
      ref="inputEl"
      v-model="content"
      type="text"
      :placeholder="$t('快速记录此刻的工作 / 生活…')"
      @keydown.enter.prevent="submit"
    />
    <div class="tags-row" :style="compact ? 'opacity:1' : ''">
      <button
        v-for="t in tagsStore.options"
        :key="t"
        class="tag-pick"
        :class="{ on: pickedTags.includes(t) }"
        @click="toggleTag(t)"
      >
        {{ t }}
      </button>
      <template v-if="addingTag">
        <input
          v-model="newTag"
          class="input tag-add-input"
          :placeholder="$t('新标签，回车确认')"
          maxlength="12"
          @keydown.enter.prevent="confirmNewTag"
          @blur="confirmNewTag"
        />
      </template>
      <button v-else class="tag-pick tag-add" :title="$t('新增自定义标签')" @click="addingTag = true">＋</button>
      <span class="hotkey">{{ $t('Enter 保存') }}</span>
      <button
        v-if="content.trim()"
        class="tag-pick ai-tag"
        :disabled="tagSuggesting"
        :title="app.aiReady ? $t('AI 从内容里提取标签') : $t('配置 AI 后可智能打标（当前按已有标签匹配）')"
        @click="suggestTags(true)"
      >
        {{ tagSuggesting ? $t('思考中…') : $t('✨ AI 打标') }}
      </button>
    </div>
  </div>
</template>
