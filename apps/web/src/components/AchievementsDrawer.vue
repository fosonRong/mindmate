<script setup lang="ts">
// 成就抽屉（FR-6.2 / 里程碑 M7）：展示全部徽章与解锁状态
import { computed, onMounted, ref } from 'vue'
import { api } from '@/api/client'
import { useAppStore } from '@/stores/app'
import type { AchievementDef } from '@/api/types'

const emit = defineEmits<{ (e: 'close'): void }>()
const app = useAppStore()
const items = ref<AchievementDef[]>([])
const loading = ref(true)

const unlockedCount = computed(() => items.value.filter((a) => a.unlocked).length)

onMounted(async () => {
  try {
    // 打开时先评估一次，保证展示最新解锁状态
    items.value = await api.checkAchievements()
  } catch {
    try {
      items.value = await api.achievements()
    } catch (e: any) {
      app.toast('error', e?.message || '加载成就失败')
    }
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="modal-mask" @click.self="emit('close')">
    <div class="modal" style="max-width: 560px">
      <div class="row">
        <h3>我的徽章</h3>
        <span class="card-sub">{{ unlockedCount }} / {{ items.length }} 已解锁</span>
        <div class="spacer"></div>
        <button class="icon-btn" @click="emit('close')">×</button>
      </div>

      <div class="small muted">所有徽章均在本地判定，数据不出本机；激励可关闭，不打扰。</div>

      <div v-if="loading" class="center muted small" style="padding: 20px">加载中…</div>
      <div v-else class="achievement-grid">
        <div
          v-for="a in items"
          :key="a.id"
          class="achievement"
          :class="{ on: a.unlocked }"
          :title="a.condition"
        >
          <div style="font-size: 24px; line-height: 1.4">{{ a.unlocked ? a.icon : '🔒' }}</div>
          <div style="font-weight: 500; margin-top: 2px">{{ a.name }}</div>
          <div class="small muted">{{ a.description }}</div>
          <div v-if="a.unlocked" class="small" style="color: var(--accent); margin-top: 4px">
            {{ (a.unlockedAt || '').slice(0, 10) }} 解锁
          </div>
          <div v-else class="small" style="color: var(--text-disable); margin-top: 4px">{{ a.condition }}</div>
        </div>
      </div>

      <div class="modal-actions">
        <button class="btn btn-primary" @click="emit('close')">知道了</button>
      </div>
    </div>
  </div>
</template>
