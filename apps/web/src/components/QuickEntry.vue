<script setup lang="ts">
// 速记输入框：页面常驻入口 —— 一次提交 = 一个节点
// v1.1.1：标签选项改为「内置 ∪ 自定义 ∪ 在用」（stores/tags），支持 AI 打标：
// 内容停顿 2.5s 自动建议（设置 ai_autotag 可关），或点 ✨ 立即建议，建议直接选中可再改。
// v1.1.2：🤖 拆待办 —— 一句话智能拆成待办（标题/日期/时间），弹窗核对后入库。
import { ref, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { useNodesStore } from '@/stores/nodes'
import { useAppStore } from '@/stores/app'
import { useTagsStore } from '@/stores/tags'
import { api } from '@/api/client'
import type { ExtractedTodo } from '@/api/types'
import SmartTodoModal from '@/components/SmartTodoModal.vue'
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

// ── 🎯 关联待办（v1.5.3）：快速记录可作为某待办的子任务/进展（OKR 结构）──
const linkedTodo = ref<{ id: number; title: string } | null>(null)
const pickingTodo = ref(false)
const todoQuery = ref('')
const todoOptions = ref<{ id: number; title: string; dueDate: string }[]>([])

async function openTodoPicker() {
  pickingTodo.value = !pickingTodo.value
  todoQuery.value = ''
  if (pickingTodo.value) await searchTodos()
}

async function searchTodos() {
  try {
    // 未完成待办优先（子任务挂在未完成事项下才有意义），关键词过滤
    const params: Record<string, string> = { status: '未完成' }
    if (todoQuery.value.trim()) params.q = todoQuery.value.trim()
    const all = await api.todos(params)
    todoOptions.value = (all as any[]).slice(0, 8).map((t) => ({ id: t.id, title: t.title, dueDate: t.dueDate }))
  } catch {
    todoOptions.value = []
  }
}

function pickTodo(t: { id: number; title: string }) {
  linkedTodo.value = { id: t.id, title: t.title }
  pickingTodo.value = false
}

function clearLinkedTodo() {
  linkedTodo.value = null
}

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

watch(content, (v) => {
  onContentInput()
  onSimilarInput()
})
// ── 相似内容提示（v1.4.1）：本地检索已有记录，轻提示防重复 ──
const similarNode = ref<{ id: number; content: string } | null>(null)
let similarTimer: ReturnType<typeof setTimeout> | null = null

async function checkSimilar(text: string) {
  if (text.length < 4) {
    similarNode.value = null
    return
  }
  try {
    const hits = await api.searchNodes(text.slice(0, 30))
    const hit = hits.find((n) => n.content.includes(text.slice(0, 8)) || text.includes(n.content.slice(0, 8)))
    similarNode.value = hit ? { id: hit.id, content: hit.content } : null
  } catch {
    similarNode.value = null
  }
}

function onSimilarInput() {
  if (similarTimer) clearTimeout(similarTimer)
  similarTimer = setTimeout(() => {
    const text = content.value.trim()
    if (text.length >= 4) checkSimilar(text)
    else similarNode.value = null
  }, 400)
}

// 用户关掉自动打标 / 配好 AI 后立即生效
watch(
  () => [app.settings.ai_autotag, app.aiReady],
  () => {
    if (suggestTimer) { clearTimeout(suggestTimer); suggestTimer = null }
  }
)

// ── 🎤 语音速记（v1.4.0）：特性探测，WebView2/浏览器可用才显示 ──
const speechAvailable = ref(false)
const listening = ref(false)
let recognition: any = null

function detectSpeech() {
  const SR = (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition
  speechAvailable.value = !!SR
}

function toggleSpeech() {
  if (listening.value) {
    recognition?.stop()
    return
  }
  const SR = (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition
  if (!SR) return
  recognition = new SR()
  recognition.lang = 'zh-CN'
  recognition.interimResults = false
  recognition.continuous = false
  recognition.onresult = (ev: any) => {
    const text = Array.from(ev.results as ArrayLike<any>)
      .map((r: any) => r[0].transcript)
      .join('')
      .trim()
    if (text) {
      content.value = content.value ? `${content.value} ${text}` : text
    }
  }
  recognition.onend = () => (listening.value = false)
  recognition.onerror = () => (listening.value = false)
  try {
    recognition.start()
    listening.value = true
  } catch {
    listening.value = false
    app.toast('error', t('语音识别启动失败（可能需要联网语音服务）'))
  }
}

/** 把当前输入的一句话拆成待办（后端 AI 优先、本地规则兜底），弹窗核对后入库 */
const extracting = ref(false)
const showExtract = ref(false)
const extractResult = ref<{ isAi: boolean; todos: ExtractedTodo[] } | null>(null)

/** 粘贴截图 → 存 captures → 直接保存为带图记录（v1.4.0，桌面/浏览器均可用） */
async function onPaste(e: ClipboardEvent) {
  const items = e.clipboardData?.items
  if (!items) return
  for (const item of items) {
    if (item.type.startsWith('image/')) {
      const blob = item.getAsFile()
      if (!blob) continue
      e.preventDefault()
      try {
        const b64 = await blobToBase64(blob)
        const ext = item.type === 'image/jpeg' ? 'jpg' : 'png'
        const resp = await api.captureImage(`paste.${ext}`, b64)
        await nodes.create(`${t('🖼️ 图片速记')}
${resp.url}`, [...pickedTags.value], props.date)
        content.value = ''
        app.toast('success', t('图片已存为记录'))
        app.refreshStats()
        emit('saved')
      } catch (err: any) {
        app.toast('error', err?.message || t('保存失败'))
      }
      return
    }
  }
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const s = String(reader.result)
      resolve(s.slice(s.indexOf(',') + 1))
    }
    reader.onerror = reject
    reader.readAsDataURL(blob)
  })
}

/** 把当前输入的一句话拆成待办（后端 AI 优先、本地规则兜底），弹窗核对后入库 */
async function extractTodos() {
  const text = content.value.trim()
  if (!text || extracting.value) return
  if (suggestTimer) { clearTimeout(suggestTimer); suggestTimer = null }
  extracting.value = true
  try {
    extractResult.value = await api.aiExtractTodos(text)
    showExtract.value = true
  } catch (e: any) {
    app.toast('error', e?.message || t('拆解失败，请稍后再试'))
  } finally {
    extracting.value = false
  }
}

/** 弹窗里确认入库：清空速记框（原句已在弹窗里用掉了） */
function onExtractSaved() {
  showExtract.value = false
  content.value = ''
  lastSuggestedFor = ''
  app.refreshStats()
  emit('saved')
}

async function submit() {
  if (!content.value.trim() || saving.value) return
  if (suggestTimer) { clearTimeout(suggestTimer); suggestTimer = null }
  saving.value = true
  try {
    await nodes.create(content.value, [...pickedTags.value], props.date, linkedTodo.value?.id ?? null)
    content.value = ''
    lastSuggestedFor = ''
    similarNode.value = null
    linkedTodo.value = null
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
  detectSpeech()
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
      :placeholder="$t('快速记录此刻的工作 / 生活…（可直接粘贴截图）')"
      @keydown.enter.prevent="submit"
      @paste="onPaste"
    />
    <div v-if="similarNode" class="similar-hint small">
      <span>{{ $t('已有相似记录：') }}{{ similarNode.content.slice(0, 24) }}{{ similarNode.content.length > 24 ? '…' : '' }}</span>
      <span class="link" @click="similarNode = null">{{ $t('仍要记录') }}</span>
    </div>
    <div class="tags-row" :style="compact ? 'opacity:1' : ''">
      <button
        class="tag-pick todo-link"
        :class="{ on: !!linkedTodo }"
        :title="$t('关联到某个待办，作为它的子任务/进展（OKR 结构）')"
        @click="openTodoPicker"
      >
        {{ linkedTodo ? `🎯 ${linkedTodo.title.slice(0, 12)}${linkedTodo.title.length > 12 ? '…' : ''}` : $t('🎯 关联待办') }}
      </button>
      <template v-if="linkedTodo">
        <span class="link small" @click="clearLinkedTodo">{{ $t('取消关联') }}</span>
      </template>
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
      <button
        v-if="content.trim()"
        class="tag-pick ai-tag"
        :disabled="tagSuggesting"
        :title="app.aiReady ? $t('AI 从内容里提取标签') : $t('配置 AI 后可智能打标（当前按已有标签匹配）')"
        @click="suggestTags(true)"
      >
        {{ tagSuggesting ? $t('思考中…') : $t('✨ AI 打标') }}
      </button>
      <button
        v-if="speechAvailable"
        class="tag-pick"
        :class="{ on: listening }"
        :title="$t('语音输入（识别为文字后可再打标/拆待办）')"
        @click="toggleSpeech"
      >
        {{ listening ? $t('🔴 听写中…') : $t('🎤 说') }}
      </button>
      <button
        v-if="content.trim()"
        class="tag-pick smart-todo"
        :disabled="extracting"
        :title="app.aiReady ? $t('AI 把这句话拆成待办（日期/时间）') : $t('按本地规则拆出日期时间（配置 AI 更准）')"
        @click="extractTodos"
      >
        {{ extracting ? $t('拆解中…') : $t('🤖 拆待办') }}
      </button>
      <span class="hotkey" :title="$t('输入内容后可一键打标，或把一句话拆成待办')">{{ $t('Enter 保存') }}</span>
    </div>

    <div v-if="pickingTodo" class="todo-picker">
      <input
        v-model="todoQuery"
        class="input"
        type="text"
        :placeholder="$t('搜索待办标题…（未完成的待办）')"
        @input="searchTodos"
        @keydown.enter.prevent="pickTodo(todoOptions[0])"
      />
      <div v-if="!todoOptions.length" class="small muted" style="padding: 6px 2px">{{ $t('没有匹配的未完成待办') }}</div>
      <div
        v-for="t in todoOptions"
        :key="t.id"
        class="todo-option small"
        @click="pickTodo(t)"
      >
        <span>{{ t.title }}</span>
        <span class="mono muted">{{ t.dueDate.slice(5) }}</span>
      </div>
    </div>

    <SmartTodoModal
      v-if="showExtract && extractResult"
      :items="extractResult.todos"
      :is-ai="extractResult.isAi"
      :source="content"
      @close="showExtract = false"
      @saved="onExtractSaved"
    />
  </div>
</template>

<style scoped>
.todo-link.on { border-color: var(--primary, #6366f1); color: var(--primary, #6366f1); }
.todo-picker {
  margin-top: 6px;
  border: 1px solid var(--border, #e5e7eb);
  border-radius: 10px;
  background: var(--bg-card, #fff);
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 240px;
  overflow-y: auto;
}
.todo-option {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 8px;
  cursor: pointer;
  word-break: break-word;
}
.todo-option:hover { background: var(--bg-soft, #f3f4f6); }
</style>
