<script setup lang="ts">
// 速记浮窗（桌面全局热键 Alt+Z 唤起的独立窗口）
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue'
import { useNodesStore } from '@/stores/nodes'
import { useAppStore, todayStr } from '@/stores/app'
import { closeQuickEntry, isDesktop } from '@/lib/desktop'

const nodes = useNodesStore()
const app = useAppStore()

const content = ref('')
const tags = ref<string[]>([])
const saved = ref(false)
const savedAt = ref('')
const inputEl = ref<HTMLInputElement | null>(null)
const TAG_OPTIONS = ['工作', '生活', '健康', '学习']

const today = todayStr()

function toggleTag(t: string) {
  const i = tags.value.indexOf(t)
  if (i >= 0) tags.value.splice(i, 1)
  else tags.value.push(t)
}

async function save() {
  const text = content.value.trim()
  if (!text) return
  try {
    await nodes.create(text, [...tags.value], today)
    savedAt.value = new Date().toTimeString().slice(0, 5)
    saved.value = true
    content.value = ''
    tags.value = []
    app.refreshStats()
    // 桌面端：0.6 秒后自动关闭窗口
    setTimeout(async () => {
      saved.value = false
      await closeWindow()
    }, 600)
  } catch (e: any) {
    app.toast('error', e?.message || '记录失败')
  }
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

/** 浮窗每次重新唤起时：清掉上次的"已记录"态并聚焦输入框 */
function onWake() {
  saved.value = false
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
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('focus', onWake)
  document.removeEventListener('visibilitychange', onVisibility)
})

const todayCount = computed(() => app.stats?.nodeCount ?? nodes.nodes.length)
const goal = computed(() => app.stats?.dailyGoal ?? 4)
</script>

<template>
  <div class="quick-window">
    <template v-if="!saved">
      <div class="head">
        <span>⏺ 速记</span>
        <span class="mono">{{ new Date().toTimeString().slice(0, 5) }}</span>
        <div class="spacer"></div>
        <span>今日 {{ todayCount }}/{{ goal }}</span>
      </div>

      <input
        ref="inputEl"
        v-model="content"
        class="quick-input"
        type="text"
        placeholder="记录此刻…"
        @keydown.enter.prevent="save"
      />

      <div class="row wrap" style="gap: 6px">
        <button
          v-for="t in TAG_OPTIONS"
          :key="t"
          class="tag-pick"
          :class="{ on: tags.includes(t) }"
          @click="toggleTag(t)"
        >
          {{ t }}
        </button>
      </div>

      <div class="row" style="margin-top: auto">
        <span class="small muted">Enter 保存 · Esc 关闭</span>
        <div class="spacer"></div>
        <button class="btn btn-sm" @click="closeWindow">关闭</button>
        <button class="btn btn-sm btn-primary" @click="save">✔ 记录</button>
      </div>
    </template>

    <template v-else>
      <div class="quick-success">
        <div class="tick">✓</div>
        <div style="font-size: 13px; font-weight: 500">已记录 {{ savedAt }}</div>
        <div class="small muted">即将自动关闭…</div>
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
</style>
