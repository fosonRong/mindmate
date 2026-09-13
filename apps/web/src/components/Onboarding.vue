<script setup lang="ts">
// 首次启动引导（里程碑 M7）：三步 —— 部署模式 → 每日目标 → AI 模型（可跳过）
import { ref } from 'vue'
import { api } from '@/api/client'
import { useAppStore } from '@/stores/app'
import { isDesktop } from '@/lib/desktop'
import type { Preset } from '@/api/types'

const emit = defineEmits<{ (e: 'done'): void }>()
const app = useAppStore()

const step = ref(0)
const total = 3

// 第 2 步：每日目标
const goalEnabled = ref(true)
const dailyGoal = ref(4)

// 第 3 步：AI 模型（可跳过）
const presets = ref<Preset[]>([])
const pickedProvider = ref<string>('')
const apiKey = ref('')
const saving = ref(false)

const isDesktopApp = isDesktop()
const modeLabel = (m?: string) => (m === 'lan' ? '局域网模式' : m === 'server' ? '服务器模式' : '本地模式')

async function loadPresets() {
  try {
    presets.value = (await api.presets()).slice(0, 4)
  } catch {
    /* 忽略：无网络或接口异常时仍可跳过 */
  }
}

function pick(p: Preset) {
  pickedProvider.value = p.id
  const def = presets.value.find((x) => x.id === p.id)
  if (def) {
    aiProvider.value = def.id
    aiBaseUrl.value = def.baseUrl
    aiModel.value = def.models[0]
  }
}
const aiProvider = ref('glm')
const aiBaseUrl = ref('')
const aiModel = ref('')

async function next() {
  if (step.value === 1) {
    await app.saveSettings({
      daily_goal: String(dailyGoal.value),
      daily_goal_enabled: goalEnabled.value ? '1' : '0'
    })
  }
  if (step.value === 2) {
    await finish(true)
    return
  }
  step.value++
  if (step.value === 2 && presets.value.length === 0) loadPresets()
}

async function finish(saveAi: boolean) {
  saving.value = true
  try {
    if (saveAi && pickedProvider.value) {
      await api.saveAiConfig({
        provider: aiProvider.value,
        baseUrl: aiBaseUrl.value,
        model: aiModel.value,
        temperature: 0.7,
        maxTokens: 2048,
        apiKey: apiKey.value || undefined
      })
      app.toast('success', 'AI 模型已配置，可随时在设置中调整')
    }
    await app.saveSettings({ onboarded: '1' })
    emit('done')
  } catch (e: any) {
    app.toast('error', e?.message || '保存失败')
  } finally {
    saving.value = false
  }
}

loadPresets()
</script>

<template>
  <div class="modal-mask">
    <div class="modal" style="max-width: 520px">
      <div class="row">
        <div class="logo" style="width: 40px; height: 40px; font-size: 18px">M</div>
        <div>
          <div style="font-size: 16px; font-weight: 600">欢迎使用智伴 Mindmate</div>
          <div class="small muted">花 30 秒完成初始化，之后随时可在设置里修改</div>
        </div>
      </div>

      <div class="row" style="gap: 6px; margin: 4px 0">
        <div
          v-for="i in total"
          :key="i"
          style="flex: 1; height: 4px; border-radius: 4px"
          :style="{ background: i - 1 <= step ? 'var(--primary)' : 'var(--bg-hover)' }"
        ></div>
      </div>

      <!-- 第 1 步：部署模式 -->
      <template v-if="step === 0">
        <div class="card-title" style="font-size: 15px">① 你的使用方式</div>
        <div class="hint-bar info">
          当前运行于 <b>{{ modeLabel(app.auth?.mode) }}</b>
          {{ isDesktopApp ? '· 桌面端已就绪（托盘常驻、Alt+Z 全局速记、系统通知）' : '· 浏览器端已就绪' }}
        </div>
        <div class="small muted">
          数据全部保存在本机（SQLite）。如需在手机/其他电脑上使用，可改为局域网模式或部署到自己的服务器 ——
          相关说明见 README 的「部署」章节。
        </div>
      </template>

      <!-- 第 2 步：每日目标 -->
      <template v-if="step === 1">
        <div class="card-title" style="font-size: 15px">② 每天想记录几条？</div>
        <div class="row">
          <span style="flex: 1; font-size: 13px">启用每日目标（驱动今日进度条）</span>
          <div class="switch" :class="{ on: goalEnabled }" @click="goalEnabled = !goalEnabled"></div>
        </div>
        <div class="form-row">
          <label class="form-label">目标条数（默认 4，可随时修改）</label>
          <div class="row">
            <input v-model.number="dailyGoal" type="number" min="1" max="50" class="input" style="width: 120px" />
            <span class="small muted">条 / 天</span>
          </div>
        </div>
        <div class="small muted">一次录入即一个节点，达标后继续记录会显示「超额」。</div>
      </template>

      <!-- 第 3 步：AI 模型 -->
      <template v-if="step === 2">
        <div class="card-title" style="font-size: 15px">③ 配置 AI 模型（可跳过）</div>
        <div class="small muted">
          配置后，晨间简报、日报/周报/月报、周度复盘与问答将由大模型生成；
          未配置时会自动使用本地模板，功能同样可用。
        </div>
        <div class="preset-grid">
          <div
            v-for="p in presets"
            :key="p.id"
            class="preset-card"
            :class="{ on: pickedProvider === p.id }"
            @click="pick(p)"
          >
            <div class="name">
              {{ p.name }}
              <span v-if="p.freeModel" class="badge free">免费</span>
            </div>
            <div class="desc">{{ p.note }}</div>
            <div class="tick">✓</div>
          </div>
        </div>
        <div v-if="pickedProvider" class="form-row">
          <label class="form-label">
            API Key
            <span class="small muted">（仅存本机系统安全存储，界面不回读）</span>
          </label>
          <input v-model="apiKey" type="password" class="input" placeholder="粘贴你的 API Key" />
        </div>
        <div class="hint-bar warn">
          ⚙️ 也可以现在跳过：在「设置 → AI 模型」中随时配置，报告会先用本地模板生成。
        </div>
      </template>

      <div class="modal-actions">
        <button class="btn" @click="finish(false)">跳过引导</button>
        <div class="spacer"></div>
        <button v-if="step > 0" class="btn" @click="step--">上一步</button>
        <button class="btn btn-primary" :disabled="saving" @click="next">
          {{ step === total - 1 ? (pickedProvider ? '完成并保存' : '完成') : '下一步' }}
        </button>
      </div>
    </div>
  </div>
</template>
