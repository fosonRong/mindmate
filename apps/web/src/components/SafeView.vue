<script setup lang="ts">
/**
 * 页面级错误边界。
 *
 * 背景：Vue 默认在子组件渲染抛错时只打印错误并把该子树渲染成空白 —— 用户看到的就是
 * 「白屏」，且没有任何可操作提示。这里捕获后代组件抛出的任何错误，改渲染一块可读的
 * 提示卡片，并允许原地重试，避免一个词条或一段数据问题毁掉整页。
 */
import { onErrorCaptured, ref } from 'vue'
import { t } from '@/i18n'

const detail = ref('')
const nonce = ref(0)

onErrorCaptured((err) => {
  detail.value = (err as Error)?.message || String(err)
  console.error('[智伴] 页面渲染出错：', err)
  return false // 已处理，不再向上冒泡
})

function retry() {
  detail.value = ''
  nonce.value++
}
</script>

<template>
  <div v-if="detail" class="card stack">
    <div class="card-title" style="font-size: 15px">{{ $t('页面渲染出错') }}</div>
    <div class="small muted">{{ $t('此页遇到一个错误，已记录到控制台。可以重试，或返回今日页继续使用。') }}</div>
    <div class="small mono muted" style="word-break: break-all">{{ detail }}</div>
    <div class="row">
      <button class="btn btn-sm btn-primary" @click="retry">{{ $t('重试') }}</button>
      <router-link to="/" class="btn btn-sm" @click="retry">{{ t('返回今日页') }}</router-link>
    </div>
  </div>
  <div v-else :key="nonce">
    <slot />
  </div>
</template>
