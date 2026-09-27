<script setup lang="ts">
// AI 对话式生成提示词模板（用户需求：模板支持与大模型对话生成，作为自定义输入）。
// 多轮迭代：每轮把历史带给模型；「应用」把草稿回填到设置页模板编辑器（是否保存由用户显式决定）。
import { nextTick, ref } from 'vue'
import { api } from '@/api/client'
import { useAppStore } from '@/stores/app'
import { t } from '@/i18n'

const props = defineProps<{ type: string }>()
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
    <div class="modal template-ai-modal">
      <div class="row" style="align-items: center">
        <h3 style="margin: 0; font-size: 16px">🤖 {{ $t('AI 生成模板') }}</h3>
        <div class="spacer"></div>
        <button class="btn btn-sm" @click="emit('close')">{{ $t('关闭') }}</button>
      </div>

      <div ref="logEl" class="tpl-ai-log">
        <div v-if="!history.length" class="small muted" style="padding: 8px 2px">
          {{ $t('用一句话描述你想要的报告模板，例如：') }}
          <span
            v-for="ex in (EXAMPLES[type] || EXAMPLES.weekly)"
            :key="ex"
            class="tpl-ai-chip"
            @click="send(ex)"
          >{{ ex }}</span>
        </div>
        <div v-for="(m, i) in history" :key="i" class="tpl-ai-msg" :class="m.role">
          <div class="tpl-ai-bubble">{{ m.content }}</div>
        </div>
        <div v-if="busy" class="small muted" style="padding: 4px 2px">{{ $t('AI 正在生成…') }}</div>
      </div>

      <div class="row" style="gap: 8px; align-items: flex-start">
        <textarea
          v-model="input"
          class="textarea"
          rows="2"
          :placeholder="$t('描述你的要求：结构、语气、侧重点…（Ctrl+Enter 发送）')"
          @keydown.ctrl.enter.prevent="send()"
          @keydown.meta.enter.prevent="send()"
        ></textarea>
        <button class="btn btn-primary" :disabled="busy || !input.trim()" @click="send()">
          {{ $t('生成') }}
        </button>
      </div>

      <div v-if="draft" class="tpl-ai-draft">
        <div class="small muted" style="margin-bottom: 4px">{{ $t('最新模板草稿（应用后回填到编辑器，点「保存模板」生效）') }}</div>
        <pre class="tpl-ai-pre">{{ draft }}</pre>
      </div>

      <div class="modal-actions" style="justify-content: flex-end">
        <button class="btn" @click="emit('close')">{{ $t('取消') }}</button>
        <button class="btn btn-primary" :disabled="!draft || busy" @click="apply">{{ $t('应用到模板') }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tpl-ai-log {
  max-height: 300px;
  min-height: 120px;
  overflow-y: auto;
  padding: 8px;
  border: 1px solid var(--border, #e5e7eb);
  border-radius: 10px;
  background: var(--bg-soft, #f9fafb);
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.tpl-ai-msg { display: flex; }
.tpl-ai-msg.user { justify-content: flex-end; }
.tpl-ai-bubble {
  max-width: 86%;
  white-space: pre-wrap;
  word-break: break-word;
  padding: 8px 10px;
  border-radius: 10px;
  font-size: 13px;
  line-height: 1.55;
  background: #fff;
  border: 1px solid var(--border, #e5e7eb);
}
.tpl-ai-msg.user .tpl-ai-bubble { background: var(--primary-soft, #eef2ff); }
.tpl-ai-chip {
  display: inline-block;
  margin: 4px 4px 0 0;
  padding: 3px 10px;
  border-radius: 999px;
  border: 1px dashed var(--border, #d1d5db);
  cursor: pointer;
  font-size: 12px;
}
.tpl-ai-chip:hover { background: #fff; }
.tpl-ai-draft { margin-top: 4px; }
.tpl-ai-pre {
  margin: 0;
  padding: 8px 10px;
  max-height: 160px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, monospace;
  font-size: 12px;
  border: 1px solid var(--border, #e5e7eb);
  border-radius: 10px;
  background: var(--bg-soft, #f9fafb);
}
</style>
