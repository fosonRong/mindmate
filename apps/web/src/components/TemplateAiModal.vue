<script setup lang="ts">
// AI 对话式生成提示词模板（用户需求：模板支持与大模型对话生成，作为自定义输入）。
// 多轮迭代：每轮把历史带给模型；「应用」把草稿回填到设置页模板编辑器（是否保存由用户显式决定）。
import { nextTick, ref } from 'vue'
import { api } from '@/api/client'
import { useAppStore } from '@/stores/app'
import { t } from '@/i18n'

const props = defineProps<{ type: string; label?: string }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'apply', content: string): void }>()
const app = useAppStore()

interface Turn { role: 'user' | 'assistant'; content: string }

const input = ref('')
const history = ref<Turn[]>([])
const draft = ref('')
const busy = ref(false)
const logEl = ref<HTMLElement | null>(null)

const EXAMPLES: Record<string, string[]> = {
  weekly: [t('按主题汇总本周工作，突出量化结果'), t('面向汇报给领导：结论先行，附风险与求助')],
  monthly: [t('按项目维度总结本月进展'), t('突出关键产出与下月计划')],
}

async function scrollLog() {
  await nextTick()
  if (logEl.value) logEl.value.scrollTop = logEl.value.scrollHeight
}

async function send(text?: string) {
  const message = (text ?? input.value).trim()
  if (!message || busy.value) return
  if (!text) input.value = ''
  history.value.push({ role: 'user', content: message })
  draft.value = ''
  busy.value = true
  scrollLog()
  try {
    const r = await api.aiTemplateDraft(props.type, message, history.value.slice(0, -1))
    const content = (r as any).content || ''
    history.value.push({ role: 'assistant', content })
    draft.value = content
  } catch (e: any) {
    app.toast('error', e?.message || t('生成失败'))
    history.value.pop()
  } finally {
    busy.value = false
    scrollLog()
  }
}

function apply() {
  if (!draft.value) return
  emit('apply', draft.value)
}
</script>

<template>
  <div class="modal-mask" @click.self="emit('close')">
    <div class="modal tpl-modal">
      <div class="tpl-head">
        <div>
          <h3>🤖 {{ $t('AI 生成模板') }}<span v-if="label" class="tpl-type">{{ label }}</span></h3>
          <div class="small muted">{{ $t('用一句话描述你想要的报告模板，例如：') }}</div>
        </div>
        <button class="btn btn-sm" @click="emit('close')">{{ $t('关闭') }}</button>
      </div>

      <div ref="logEl" class="tpl-log">
        <template v-if="!history.length">
          <div class="tpl-chip" v-for="ex in (EXAMPLES[type] || EXAMPLES.weekly)" :key="ex" @click="send(ex)">{{ ex }}</div>
        </template>
        <div v-for="(m, i) in history" :key="i" class="tpl-msg" :class="m.role">
          <div class="tpl-bubble">{{ m.content }}</div>
        </div>
        <div v-if="busy" class="small muted tpl-typing"><span class="tpl-dot"></span>{{ $t('AI 正在生成…') }}</div>
      </div>

      <div class="tpl-input-row">
        <textarea
          v-model="input"
          class="textarea"
          rows="3"
          :placeholder="$t('描述你的要求：结构、语气、侧重点…（Ctrl+Enter 发送）')"
          @keydown.ctrl.enter.prevent="send()"
          @keydown.meta.enter.prevent="send()"
        ></textarea>
        <button class="btn btn-primary tpl-send" :disabled="busy || !input.trim()" @click="send()">
          {{ busy ? $t('生成中…') : $t('生成') }}
        </button>
      </div>

      <div v-if="draft" class="tpl-draft">
        <div class="small muted tpl-draft-label">
          <span>{{ $t('最新模板草稿（应用后回填到编辑器，点「保存模板」生效）') }}</span>
        </div>
        <pre class="tpl-pre">{{ draft }}</pre>
      </div>

      <div class="modal-actions">
        <button class="btn" @click="emit('close')">{{ $t('取消') }}</button>
        <button class="btn btn-primary" :disabled="!draft || busy" @click="apply">{{ $t('应用到模板') }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tpl-modal { max-width: 660px; width: 100%; }
.tpl-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
.tpl-head h3 { margin: 0; font-size: 16px; display: flex; align-items: center; gap: 8px; }
.tpl-type {
  font-size: 12px; font-weight: 600; padding: 2px 10px; border-radius: 999px;
  background: var(--primary-weak); color: var(--primary);
}
.tpl-log {
  height: 300px;
  overflow-y: auto;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--bg-page);
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.tpl-chip {
  align-self: flex-start;
  max-width: 90%;
  padding: 8px 12px;
  border-radius: 999px;
  border: 1px dashed var(--border-strong);
  cursor: pointer;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-regular);
  background: var(--bg-card);
}
.tpl-chip:hover { border-color: var(--primary); color: var(--primary); }
.tpl-msg { display: flex; }
.tpl-msg.user { justify-content: flex-end; }
.tpl-bubble {
  max-width: 86%;
  white-space: pre-wrap;
  word-break: break-word;
  padding: 8px 12px;
  border-radius: 12px;
  font-size: 13px;
  line-height: 1.6;
  background: var(--bg-card);
  border: 1px solid var(--border);
  color: var(--text-strong);
}
.tpl-msg.user .tpl-bubble { background: var(--primary-weak); border-color: transparent; }
.tpl-typing { display: flex; align-items: center; gap: 6px; padding: 2px; }
.tpl-dot {
  width: 8px; height: 8px; border-radius: 50%; background: var(--primary);
  animation: tpl-pulse 1s ease-in-out infinite;
}
@keyframes tpl-pulse { 0%, 100% { opacity: .3; } 50% { opacity: 1; } }
.tpl-input-row { display: flex; gap: 10px; align-items: stretch; }
.tpl-input-row .textarea { flex: 1; resize: none; }
.tpl-send { white-space: nowrap; min-width: 92px; }
.tpl-draft { display: flex; flex-direction: column; gap: 6px; }
.tpl-draft-label { display: flex; justify-content: space-between; gap: 8px; }
.tpl-pre {
  margin: 0;
  padding: 10px 12px;
  max-height: 200px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, monospace;
  font-size: 12px;
  line-height: 1.6;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--bg-page);
  color: var(--text-strong);
}
</style>
