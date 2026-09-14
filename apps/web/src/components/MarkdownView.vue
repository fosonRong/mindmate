<script setup lang="ts">
// Markdown 渲染（DOMPurify 清洗，防 XSS）
// 解析前先剥掉包裹全文的 ```markdown 围栏（模型偶发行为，见 lib/markdown.ts）
import { computed } from 'vue'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import { unwrapMarkdownFence } from '@/lib/markdown'

const props = defineProps<{ content: string }>()

marked.setOptions({ breaks: true, gfm: true })

const html = computed(() => {
  const raw = marked.parse(unwrapMarkdownFence(props.content || ''), { async: false }) as string
  return DOMPurify.sanitize(raw, { ALLOWED_TAGS: undefined, ADD_ATTR: ['target'] })
})
</script>

<template>
  <div class="md" v-html="html"></div>
</template>
