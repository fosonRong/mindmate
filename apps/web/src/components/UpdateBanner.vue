<script setup lang="ts">
// 新版本横幅：发现更新时出现在右下角，可立即更新 / 跳过此版本 / 稍后
import { useUpdateStore } from '@/stores/update'

const update = useUpdateStore()
</script>

<template>
  <div v-if="update.available" class="update-wrap">
    <div class="update-card">
      <div class="row" style="gap: 8px; align-items: baseline">
        <b style="font-size: 13px">发现新版本 v{{ update.available.version }}</b>
        <span class="small muted">当前 v{{ update.currentVersion }}</span>
      </div>
      <div v-if="update.available.notes" class="update-notes small muted">{{ update.available.notes }}</div>

      <div v-if="update.installing" class="update-progress">
        <div class="bar"><i :style="{ width: (update.progress || 3) + '%' }"></i></div>
        <span class="small muted">
          {{ update.progress ? `下载中 ${update.progress}%` : '正在下载…' }} · 完成后自动重启
        </span>
      </div>

      <div class="row" style="gap: 6px; margin-top: 8px">
        <button class="btn btn-sm btn-primary" :disabled="update.installing" @click="update.install()">
          {{ update.installing ? '更新中…' : '立即更新' }}
        </button>
        <button class="btn btn-sm" :disabled="update.installing" @click="update.skip()">跳过此版本</button>
        <div class="spacer"></div>
        <button class="icon-btn" style="width: 22px; height: 22px" :disabled="update.installing" @click="update.dismiss()">×</button>
      </div>
      <div v-if="update.error" class="small" style="color: var(--danger); margin-top: 4px">{{ update.error }}</div>
    </div>
  </div>
</template>

<style scoped>
.update-wrap {
  position: fixed;
  right: 20px;
  bottom: 20px;
  z-index: 40;
}
.update-card {
  width: 320px;
  background: var(--bg-card);
  border: 1px solid var(--border);
  border-radius: 12px;
  box-shadow: var(--shadow-pop);
  padding: 12px 14px;
}
.update-notes {
  margin-top: 6px;
  max-height: 84px;
  overflow-y: auto;
  white-space: pre-wrap;
}
.update-progress {
  margin-top: 8px;
}
.update-progress .bar {
  height: 6px;
  border-radius: 99px;
  background: var(--bg-hover);
  overflow: hidden;
}
.update-progress .bar i {
  display: block;
  height: 100%;
  background: var(--primary);
  transition: width 0.2s;
}
</style>
