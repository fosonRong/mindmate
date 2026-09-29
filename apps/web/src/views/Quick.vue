<script setup lang="ts">
// 万能捕捉浮窗（v1.4.0）：桌面全局热键 Alt+Z 唤起的独立窗口。
// 一行输入 → 智能分流：含日期/任务语义 → 待办草稿（内联确认）；链接 → 记录；普通 → 记录；拿不准 → 收集箱。
// 还支持：剪贴板一键粘贴（文本/截图）、图片粘贴 + 本机 OCR、文件拖入（桌面端）。
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue'
import { useNodesStore } from '@/stores/nodes'
import { useTodosStore } from '@/stores/todos'
import { useAppStore, todayStr } from '@/stores/app'
import { api } from '@/api/client'
import type { ExtractedTodo } from '@/api/types'
import { closeQuickEntry, isDesktop, invoke } from '@/lib/desktop'
import { t } from '@/i18n'

const nodes = useNodesStore()
const todos = useTodosStore()
const app = useAppStore()

const content = ref('')
const tags = ref<string[]>([])
const saved = ref(false)
const savedAt = ref('')
const savedKind = ref('')
const inputEl = ref<HTMLInputElement | null>(null)
const TAG_OPTIONS = ['工作', '生活', '健康', '学习']

// ── 🎯 关联待办（v1.5.3）：记录可作为某待办的子任务/进展（OKR 结构）──
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
    // 状态值需客户端过滤（后端 status 精确匹配：待处理/进行中/已完成/已逾期）
    const params: Record<string, string> = {}
    if (todoQuery.value.trim()) params.q = todoQuery.value.trim()
    const all = await api.todos(params)
    todoOptions.value = (all as any[])
      .filter((x) => x.status !== '已完成')
      .slice(0, 8)
      .map((x) => ({ id: x.id, title: x.title, dueDate: x.dueDate }))
  } catch {
    todoOptions.value = []
  }
}

function pickTodo(x?: { id: number; title: string }) {
  if (!x) return
  linkedTodo.value = { id: x.id, title: x.title }
  pickingTodo.value = false
}

function clearLinkedTodo() {
  linkedTodo.value = null
}

// ── 智能分流（v1.4.0）──
const routing = ref(false)
const draftTodos = ref<ExtractedTodo[]>([])
const draftChecked = ref<boolean[]>([])
const draftIsAi = ref(false)

// ── 剪贴板 / 图片（桌面端）──
const clipText = ref('')
const clipImage = ref<{ path: string; url: string; width: number; height: number } | null>(null)
const ocrRunning = ref(false)
const ocrText = ref('')
const desktop = isDesktop()

const today = todayStr()

function toggleTag(t: string) {
  const i = tags.value.indexOf(t)
  if (i >= 0) tags.value.splice(i, 1)
  else tags.value.push(t)
}

function isUrl(text: string) {
  return /^https?:\/\/\S+$/.test(text.trim())
}

/** 保存为记录（含捕捉图片行与来源标记） */
async function saveNode(finalText: string, from = '') {
  const text = finalText.trim()
  if (!text) return
  await nodes.create(text, [...tags.value], today, linkedTodo.value?.id ?? null)
  finishCapture(from || t('已记录'))
}

/** 收进收集箱（未排期池） */
async function collectInbox() {
  const text = content.value.trim()
  if (!text) return
  try {
    await todos.collect(text.slice(0, 100))
    finishCapture(t('已收进收集箱'))
  } catch (e: any) {
    app.toast('error', e?.message || t('操作失败'))
  }
}

/** 智能分流：URL 直接存链接记录；否则问 AI/规则拆待办，拆出即内联确认，拆不出直接存记录 */
async function smartRoute() {
  const text = content.value.trim()
  if (!text || routing.value) return
  // 链接捕捉：URL 单独成条直接存
  if (isUrl(text)) {
    await saveNode(`🔗 ${text}`, t('已存链接'))
    return
  }
  routing.value = true
  try {
    const r = await api.aiExtractTodos(text)
    const items = (r.todos || []).filter((x) => x.title.trim())
    if (items.length) {
      draftTodos.value = items
      draftChecked.value = items.map(() => true)
      draftIsAi.value = r.isAi
    } else {
      await saveNode(text)
    }
  } catch {
    // 拆解失败不阻塞：退化为普通记录
    await saveNode(text)
  } finally {
    routing.value = false
  }
}

function toggleDraft(i: number) {
  draftChecked.value[i] = !draftChecked.value[i]
}

/** 确认待办草稿入库 */
async function confirmDraft() {
  const picked = draftTodos.value.filter((_, i) => draftChecked.value[i])
  if (!picked.length) return
  try {
    for (const x of picked) {
      await todos.create({
        title: x.title.slice(0, 100),
        description: t('来自捕捉：{a}', { a: content.value.trim().slice(0, 80) }),
        dueDate: x.date,
        dueTime: x.time || null,
        tags: [t('捕捉')]
      })
    }
    finishCapture(t('已添加 {a} 条待办', { a: picked.length }))
  } catch (e: any) {
    app.toast('error', e?.message || t('保存失败'))
  }
}

/** 放弃待办草稿，改存为一条记录 */
async function draftAsRecord() {
  const text = content.value.trim()
  draftTodos.value = []
  await saveNode(text)
}

function finishCapture(kind: string) {
  savedAt.value = new Date().toTimeString().slice(0, 5)
  savedKind.value = kind
  saved.value = true
  content.value = ''
  tags.value = []
  draftTodos.value = []
  clipText.value = ''
  clipImage.value = null
  ocrText.value = ''
  app.refreshStats()
  // 0.8 秒后自动关闭
  setTimeout(async () => {
    saved.value = false
    await closeWindow()
  }, 800)
}

// ── 剪贴板（桌面端）：文本一键填入；截图一键存图 ──
async function loadClipboard() {
  if (!desktop) return
  const text = await invoke<string | null>('capture_clipboard_text')
  if (text && text.trim() && text.trim() !== content.value.trim() && text.length < 2000) {
    clipText.value = text.trim()
  }
  const img = await invoke<{ path: string; url: string; width: number; height: number } | null>(
    'capture_clipboard_image'
  ).catch(() => null)
  // capture_clipboard_image 会消费剪贴板内容吗？arboard get_image 不会清除剪贴板；
  // 仅当剪贴板真有图片时才返回条目
  if (img) clipImage.value = img
}

function useClipText() {
  if (!clipText.value) return
  content.value = clipText.value
  clipText.value = ''
  nextTick(() => inputEl.value?.focus())
}

/** 剪贴板截图 → 存图 → 直接保存为带图的记录 */
async function useClipImage() {
  if (!clipImage.value) return
  const img = clipImage.value
  try {
    await nodes.create(`${t('🖼️ 图片速记')}\n${img.url}`, [...tags.value], today)
    finishCapture(t('截图已存为记录'))
  } catch (e: any) {
    app.toast('error', e?.message || t('保存失败'))
  }
}

/** 对已存截图跑本机 OCR，识别文本填回输入框（可再编辑/分流） */
async function runOcr() {
  if (!clipImage.value || ocrRunning.value) return
  ocrRunning.value = true
  try {
    const text = await invoke<string>('ocr_image', { path: clipImage.value.path })
    if (text) {
      ocrText.value = ''
      content.value = content.value ? `${content.value}\n${text}` : text
      clipImage.value = null
      nextTick(() => inputEl.value?.focus())
    }
  } catch (e: any) {
    app.toast('error', e?.message || t('OCR 不可用'))
  } finally {
    ocrRunning.value = false
  }
}

// ── 文件拖入（桌面端）：图片文件存档，其他文件存路径 ──
async function onDropFiles(paths: string[]) {
  if (!paths.length) return
  const imgExts = ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp']
  try {
    for (const p of paths.slice(0, 3)) {
      const name = p.split(/[\\/]/).pop() || p
      const ext = (name.split('.').pop() || '').toLowerCase()
      if (imgExts.includes(ext)) {
        const saved2 = await invoke<{ path: string; url: string }>('save_capture_file', { src: p })
        if (saved2) {
          await nodes.create(`${t('🖼️ 图片速记')}\n${saved2.url}`, [...tags.value], today)
          finishCapture(t('图片已存为记录'))
        }
      } else {
        await nodes.create(`${t('📎')} ${name}\n${p}`, [...tags.value], today)
        finishCapture(t('文件已存为记录'))
      }
    }
  } catch (e: any) {
    app.toast('error', e?.message || t('保存失败'))
  }
}

// Tauri v2 文件拖放事件（桌面端）
let unlistenDrop: (() => void) | null = null

async function listenFileDrop() {
  if (!desktop) return
  try {
    const webview = await import('@tauri-apps/api/webview')
    const { getCurrentWebview } = webview
    unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'drop') {
        onDropFiles(event.payload.paths as string[])
      }
    })
  } catch (e) {
    console.warn('[mindmate] 文件拖放监听失败', e)
  }
}

/** 输入框内粘贴图片（桌面端）：转存 captures */
async function onPaste(e: ClipboardEvent) {
  if (!desktop) return
  const items = e.clipboardData?.items
  if (!items) return
  for (const item of items) {
    if (item.type.startsWith('image/')) {
      const blob = item.getAsFile()
      if (!blob) continue
      e.preventDefault()
      const b64 = await blobToBase64(blob)
      const ext = item.type === 'image/jpeg' ? 'jpg' : 'png'
      try {
        const resp = await api.captureImage(`paste.${ext}`, b64)
        await nodes.create(`${t('🖼️ 图片速记')}\n${resp.url}`, [...tags.value], today)
        finishCapture(t('图片已存为记录'))
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

/**
 * 收起浮窗。
 * 桌面端只走 close_quick_entry（隐藏窗口，WebView 保活）；
 * 绝不能用 window.close() —— Windows/WebView2 上它会销毁网页视图，
 * 只留下一个没有内容、没有按钮、按 Esc 也无反应的白色空窗（无法关闭）。
 */
async function closeWindow() {
  if (isDesktop()) {
    await closeQuickEntry()
    return
  }
  // 浏览器里直接打开 /#/quick 时的退路：回到今日页
  location.hash = '#/'
}

/** 浮窗每次重新唤起时：清掉上次状态并聚焦 + 探测剪贴板 */
function onWake() {
  saved.value = false
  loadClipboard().catch(() => {})
  nextTick(() => inputEl.value?.focus())
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    closeWindow()
  }
}

function onVisibility() {
  if (!document.hidden) onWake()
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('focus', onWake)
  document.addEventListener('visibilitychange', onVisibility)
  await nodes.load(today).catch(() => {})
  await app.refreshStats().catch(() => {})
  await nextTick()
  inputEl.value?.focus()
  loadClipboard().catch(() => {})
  listenFileDrop()
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('focus', onWake)
  document.removeEventListener('visibilitychange', onVisibility)
  unlistenDrop?.()
  unlistenDrop = null
})

const todayCount = computed(() => app.stats?.nodeCount ?? nodes.nodes.length)
const goal = computed(() => app.stats?.dailyGoal ?? 4)
const pickedCount = computed(() => draftChecked.value.filter(Boolean).length)
</script>

<template>
  <div class="quick-window">
    <template v-if="!saved">
      <div class="head">
        <span>{{ $t('⚡ 万能捕捉') }}</span>
        <span class="mono">{{ new Date().toTimeString().slice(0, 5) }}</span>
        <div class="spacer"></div>
        <span>{{ $t('今日 {a}/{b}', { a: todayCount, b: goal }) }}</span>
      </div>

      <!-- 待办草稿内联确认（智能分流结果） -->
      <div v-if="draftTodos.length" class="draft-box">
        <div class="small muted" style="margin-bottom: 4px">
          {{ draftIsAi ? $t('AI 拆出了待办，确认后入库：') : $t('拆出了待办，确认后入库：') }}
        </div>
        <label v-for="(x, i) in draftTodos" :key="i" class="draft-row">
          <input type="checkbox" :checked="draftChecked[i]" @change="toggleDraft(i)" />
          <span class="draft-title">{{ x.title }}</span>
          <span class="mono small muted">{{ x.date.slice(5) }}{{ x.time ? ' ' + x.time : '' }}</span>
        </label>
        <div class="row" style="gap: 6px; margin-top: 8px">
          <button class="btn btn-sm" @click="draftAsRecord">{{ $t('改存为记录') }}</button>
          <div class="spacer"></div>
          <button class="btn btn-sm btn-primary" :disabled="!pickedCount" @click="confirmDraft">
            {{ $t('添加选中的 {a} 条', { a: pickedCount }) }}
          </button>
        </div>
      </div>

      <template v-else>
        <input
          ref="inputEl"
          v-model="content"
          class="quick-input"
          type="text"
          :placeholder="$t('一句话：事情/日期/链接/想法…')"
          @keydown.enter.prevent="smartRoute"
          @paste="onPaste"
        />

        <!-- 剪贴板探测 chips（桌面端） -->
        <div v-if="clipImage" class="clip-chip">
          🖼️ {{ $t('剪贴板有截图') }}
          <button class="btn btn-sm btn-primary" :disabled="ocrRunning" @click="useClipImage">
            {{ $t('存为记录') }}
          </button>
          <button class="btn btn-sm" :disabled="ocrRunning" @click="runOcr">
            {{ ocrRunning ? $t('识别中…') : $t('🔍 识别文字') }}
          </button>
        </div>
        <div v-else-if="clipText" class="clip-chip">
          📋 {{ clipText.slice(0, 26) }}{{ clipText.length > 26 ? '…' : '' }}
          <button class="btn btn-sm" @click="useClipText">{{ $t('粘贴') }}</button>
        </div>

        <div class="row wrap" style="gap: 6px">
          <button
            class="tag-pick"
            :class="{ on: !!linkedTodo }"
            :title="$t('关联到某个待办，作为它的子任务/进展（OKR 结构）')"
            @click="openTodoPicker"
          >
            {{ linkedTodo ? `🎯 ${linkedTodo.title.slice(0, 10)}${linkedTodo.title.length > 10 ? '…' : ''}` : $t('🎯 关联待办') }}
          </button>
          <span v-if="linkedTodo" class="link small" @click="clearLinkedTodo">{{ $t('取消关联') }}</span>
          <button
            v-for="t in TAG_OPTIONS"
            :key="t"
            class="tag-pick"
            :class="{ on: tags.includes(t) }"
            @click="toggleTag(t)"
          >
            {{ t }}
          </button>
          <div class="spacer"></div>
          <button class="btn btn-sm" :title="$t('不打日期，之后拖进日历排期')" @click="collectInbox">
            {{ $t('📥 收集箱') }}
          </button>
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
          <div v-if="!todoOptions.length" class="small muted" style="padding: 4px 2px">{{ $t('没有匹配的未完成待办') }}</div>
          <div v-for="x in todoOptions" :key="x.id" class="todo-option small" @click="pickTodo(x)">
            <span>{{ x.title }}</span>
            <span class="mono muted">{{ x.dueDate.slice(5) }}</span>
          </div>
        </div>
      </template>

      <div class="row" style="margin-top: auto">
        <span class="small muted">{{ $t('Enter 智能分流 · Esc 关闭 · 可拖入文件/图片') }}</span>
        <div class="spacer"></div>
        <button class="btn btn-sm" @click="closeWindow">{{ $t('关闭') }}</button>
      </div>
    </template>

    <template v-else>
      <div class="quick-success">
        <div class="tick">✓</div>
        <div style="font-size: 13px; font-weight: 500">{{ savedKind }} · {{ savedAt }}</div>
        <div class="small muted">{{ $t('即将自动关闭…') }}</div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.quick-input {
  height: 44px;
  border: 1.5px solid var(--border);
  border-radius: 10px;
  background: var(--bg-hover);
  padding: 0 12px;
  font-size: 14px;
  color: var(--text-strong);
  outline: none;
  font-family: inherit;
}
.quick-input:focus {
  border-color: var(--primary);
  background: var(--bg-card);
  box-shadow: 0 0 0 3px var(--primary-weak);
}
.draft-box {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 8px 10px;
  background: var(--bg-hover);
}
.draft-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 0;
  cursor: pointer;
}
.draft-title {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  color: var(--text-strong);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.clip-chip {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-regular);
  border: 1px dashed var(--border);
  border-radius: 8px;
  padding: 5px 8px;
  background: var(--bg-hover);
}
.todo-picker {
  margin-top: 6px;
  border: 1px solid var(--border, #e5e7eb);
  border-radius: 10px;
  background: var(--bg-card, #fff);
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 180px;
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
