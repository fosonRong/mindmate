<script setup lang="ts">
// 智伴 AI：报告（日/周/月/历史）+ 复盘 + 问答
import { computed, onMounted, ref, nextTick } from 'vue'
import { api, streamApi } from '@/api/client'
import { useAppStore, todayStr, friendlyDate } from '@/stores/app'
import type { ChatMessage, Report } from '@/api/types'
import MarkdownView from '@/components/MarkdownView.vue'

const app = useAppStore()

type Tab = 'report' | 'review' | 'chat'
const tab = ref<Tab>('report')

// ── 报告 ──
type RType = 'daily' | 'weekly' | 'monthly'
const rtype = ref<RType>('daily')
const reportDate = ref(todayStr())
const reportContent = ref('')
const reportLoading = ref(false)
const reportDegraded = ref(false)
const reportElapsed = ref('')
const history = ref<Report[]>([])
let abortReport: (() => void) | null = null

const rtypeLabel: Record<RType, string> = { daily: '日报', weekly: '周报', monthly: '月报' }

async function loadHistory() {
  history.value = await api.reports(rtype.value === 'daily' ? 'daily' : rtype.value)
}

async function loadSaved() {
  const list = await api.reports(rtype.value)
  const key = rtype.value === 'daily' ? reportDate.value : undefined
  const found = key ? list.find((r) => r.period === key) : list[0]
  reportContent.value = found?.content || ''
  reportDegraded.value = found ? !found.isAi : false
  reportElapsed.value = found ? found.createdAt : ''
}

function generate() {
  if (reportLoading.value) return
  reportLoading.value = true
  reportContent.value = ''
  reportDegraded.value = false
  const started = Date.now()
  abortReport = streamApi.report(rtype.value, reportDate.value, {
    onDelta: (t, degraded) => {
      reportContent.value += t
      if (degraded) reportDegraded.value = true
    },
    onDone: () => {
      reportLoading.value = false
      reportElapsed.value = `${((Date.now() - started) / 1000).toFixed(1)}s`
      loadHistory().catch(() => {})
    },
    onError: (msg) => {
      reportLoading.value = false
      app.toast('error', msg)
    }
  })
}

function stop() {
  abortReport?.()
  reportLoading.value = false
  app.toast('info', '已停止生成')
}

async function copyReport() {
  try {
    await navigator.clipboard.writeText(reportContent.value)
    app.toast('success', '已复制 Markdown')
  } catch {
    app.toast('error', '复制失败，请手动选择')
  }
}

function exportReport() {
  const blob = new Blob([reportContent.value], { type: 'text/markdown;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${rtypeLabel[rtype.value]}-${reportDate.value}.md`
  a.click()
  URL.revokeObjectURL(url)
}

// ── 复盘 ──
const reviewContent = ref('')
const reviewLoading = ref(false)
const reviewAt = ref('')
let abortReview: (() => void) | null = null

const insights = computed(() => {
  const lines = reviewContent.value.split('\n')
  const out: { main: string; sub: string }[] = []
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i].trim()
    if (/^[-*]\s+/.test(l) || /^\d+\.\s+/.test(l)) {
      const text = l.replace(/^[-*]\s+/, '').replace(/^\d+\.\s+/, '')
      const next = (lines[i + 1] || '').trim()
      const sub = /^[-*]\s+/.test(next) && !/^[-*]\s+\*\*/.test(next) ? '' : ''
      out.push({ main: text.replace(/\*\*/g, ''), sub })
    }
  }
  return out.slice(0, 6)
})

async function loadReview() {
  const list = await api.reports('review')
  if (list[0]) {
    reviewContent.value = list[0].content
    reviewAt.value = list[0].createdAt
  }
}

function generateReview() {
  if (reviewLoading.value) return
  reviewLoading.value = true
  reviewContent.value = ''
  abortReview = streamApi.review(todayStr(), {
    onDelta: (t) => {
      reviewContent.value += t
    },
    onDone: () => {
      reviewLoading.value = false
      reviewAt.value = new Date().toLocaleString()
    },
    onError: (msg) => {
      reviewLoading.value = false
      app.toast('error', msg)
    }
  })
}

// ── 问答 ──
const question = ref('')
const messages = ref<ChatMessage[]>([])
const chatLoading = ref(false)
const chatScroll = ref<HTMLElement | null>(null)
let abortChat: (() => void) | null = null

async function loadChat() {
  messages.value = await api.chatHistory()
  await nextTick()
  scrollChat()
}

function scrollChat() {
  nextTick(() => {
    if (chatScroll.value) chatScroll.value.scrollTop = chatScroll.value.scrollHeight
  })
}

function ask() {
  const q = question.value.trim()
  if (!q || chatLoading.value) return
  question.value = ''
  messages.value.push({
    id: Date.now(),
    sessionId: 'default',
    role: 'user',
    content: q,
    createdAt: new Date().toISOString()
  })
  const reply: ChatMessage = {
    id: Date.now() + 1,
    sessionId: 'default',
    role: 'assistant',
    content: '',
    createdAt: new Date().toISOString()
  }
  messages.value.push(reply)
  chatLoading.value = true
  scrollChat()

  abortChat = streamApi.chat(q, {
    onDelta: (t) => {
      reply.content += t
      messages.value = [...messages.value]
      scrollChat()
    },
    onDone: () => {
      chatLoading.value = false
    },
    onError: (msg) => {
      chatLoading.value = false
      reply.content = reply.content || `⚠️ ${msg}`
      messages.value = [...messages.value]
    }
  })
}

async function clearChat() {
  await api.clearChat()
  messages.value = []
  app.toast('info', '已清空对话')
}

const SUGGESTIONS = ['本周完成了几件待办？', '我周三记了什么？', '哪类任务最容易拖延？', '今天有哪些逾期待办？']

onMounted(async () => {
  await Promise.all([loadSaved(), loadHistory(), loadReview(), loadChat()])
})
</script>

<template>
  <div class="col-stack">
    <div class="tabs">
      <button :class="{ on: tab === 'report' }" @click="tab = 'report'">报告</button>
      <button :class="{ on: tab === 'review' }" @click="tab = 'review'">复盘</button>
      <button :class="{ on: tab === 'chat' }" @click="tab = 'chat'">问答</button>
    </div>

    <!-- 报告 -->
    <section v-if="tab === 'report'" class="card">
      <div class="row wrap" style="margin-bottom: 12px">
        <div class="seg">
          <button v-for="t in (['daily', 'weekly', 'monthly'] as RType[])" :key="t" :class="{ on: rtype === t }" @click="rtype = t; loadSaved(); loadHistory()">
            {{ rtypeLabel[t] }}
          </button>
        </div>
        <input v-model="reportDate" type="date" class="input" style="width: 148px; height: 34px" @change="loadSaved" />
        <span class="small muted">{{ friendlyDate(reportDate) }}</span>
        <div class="spacer"></div>
        <router-link to="/settings" class="btn btn-sm">⚙️ 模型配置</router-link>
        <button v-if="reportLoading" class="btn btn-sm" @click="stop">停止生成</button>
        <button v-else class="btn btn-sm btn-primary" @click="generate">✨ 生成报告</button>
      </div>

      <div v-if="reportDegraded" class="hint-bar warn" style="margin-bottom: 12px">
        ⚙️ 当前为本地模板拼装，配置 AI 模型可获得更优质的{{ rtypeLabel[rtype] }}
        <router-link to="/settings" class="link">去配置</router-link>
      </div>

      <div class="report-box">
        <div v-if="!reportContent && !reportLoading" class="empty">
          <div class="ill">✨</div>
          <div class="t">报告从这里开始</div>
          <div class="d">基于你录入的节点与待办完成情况自动生成</div>
          <button class="btn btn-primary" style="margin-top: 6px" @click="generate">生成{{ rtypeLabel[rtype] }}</button>
        </div>
        <template v-else>
          <MarkdownView :content="reportContent" />
          <span v-if="reportLoading" class="stream-cursor"></span>
        </template>
      </div>

      <div v-if="reportContent" class="row wrap" style="margin-top: 12px">
        <span class="small muted">{{ reportElapsed ? `生成于 ${reportElapsed}` : '' }}</span>
        <div class="spacer"></div>
        <button class="btn btn-sm" @click="copyReport">📋 复制 MD</button>
        <button class="btn btn-sm" @click="exportReport">⬇️ 导出</button>
      </div>

      <!-- 历史 -->
      <div v-if="history.length" style="margin-top: 16px">
        <div class="card-title" style="font-size: 14px; margin-bottom: 8px">历史{{ rtypeLabel[rtype] }}</div>
        <div class="stack sm">
          <div v-for="h in history.slice(0, 8)" :key="h.id" class="row" style="gap: 8px; font-size: 13px">
            <span class="mono small muted">{{ h.createdAt.slice(0, 16) }}</span>
            <span class="link" @click="reportContent = h.content; reportDegraded = !h.isAi">{{ h.period }}</span>
            <span v-if="!h.isAi" class="badge">本地</span>
            <div class="spacer"></div>
            <button class="icon-btn" style="width: 24px; height: 24px" @click="api.deleteReport(h.id).then(loadHistory)">×</button>
          </div>
        </div>
      </div>
    </section>

    <!-- 复盘 -->
    <section v-if="tab === 'review'" class="card">
      <div class="row" style="margin-bottom: 12px">
        <div class="card-title" style="font-size: 15px">周度智能复盘</div>
        <span v-if="reviewAt" class="card-sub">{{ reviewAt }}</span>
        <div class="spacer"></div>
        <button v-if="reviewLoading" class="btn btn-sm" @click="abortReview?.(); reviewLoading = false">停止</button>
        <button v-else class="btn btn-sm btn-primary" @click="generateReview">📊 {{ reviewContent ? '重新生成' : '生成本周复盘' }}</button>
      </div>

      <div v-if="!reviewContent && !reviewLoading" class="empty">
        <div class="ill">📊</div>
        <div class="t">还没有复盘</div>
        <div class="d">智伴会分析本周的记录节奏、完成率与拖延规律</div>
      </div>

      <template v-else>
        <div v-if="insights.length" class="stack" style="margin-bottom: 14px">
          <div v-for="(ins, i) in insights" :key="i" class="insight-card">
            <div class="mark">✦</div>
            <div>
              <div class="main">{{ ins.main }}</div>
              <div v-if="ins.sub" class="sub">{{ ins.sub }}</div>
            </div>
          </div>
        </div>
        <details :open="!insights.length">
          <summary class="small muted pointer">查看完整复盘内容</summary>
          <div style="margin-top: 10px">
            <MarkdownView :content="reviewContent" />
            <span v-if="reviewLoading" class="stream-cursor"></span>
          </div>
        </details>
      </template>
    </section>

    <!-- 问答 -->
    <section v-if="tab === 'chat'" class="card" style="display: flex; flex-direction: column; height: calc(100vh - 210px)">
      <div class="row" style="margin-bottom: 10px">
        <div class="card-title" style="font-size: 15px">智伴问答</div>
        <span class="card-sub">基于你自己的本地数据</span>
        <div class="spacer"></div>
        <button class="btn btn-sm" @click="clearChat">清空</button>
      </div>

      <div ref="chatScroll" class="chat-scroll">
        <div v-if="messages.length === 0" class="empty">
          <div class="ill">💬</div>
          <div class="t">问我关于你记录的任何事</div>
          <div class="d">例如：本周完成了几件待办？</div>
          <div class="row wrap" style="margin-top: 8px; justify-content: center">
            <button v-for="s in SUGGESTIONS" :key="s" class="btn btn-sm" @click="question = s; ask()">{{ s }}</button>
          </div>
        </div>
        <div v-for="m in messages" :key="m.id" :class="m.role === 'user' ? 'chat-user' : 'chat-ai'">
          <template v-if="m.role === 'user'">{{ m.content }}</template>
          <template v-else>
            <MarkdownView :content="m.content" />
            <span v-if="chatLoading && m.id === messages[messages.length - 1]?.id" class="stream-cursor"></span>
          </template>
        </div>
      </div>

      <div class="quick-entry" style="margin-top: 10px">
        <input v-model="question" type="text" placeholder="问我：这周哪类任务最容易拖延？" @keydown.enter="ask" />
        <div class="tags-row" style="opacity: 1">
          <span class="small muted">Enter 发送 · 数据不出本机</span>
          <div class="spacer"></div>
          <button class="btn btn-sm btn-primary" :disabled="chatLoading" @click="ask">
            {{ chatLoading ? '思考中…' : '发送' }}
          </button>
        </div>
      </div>
    </section>
  </div>
</template>
