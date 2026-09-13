<script setup lang="ts">
// Markdown 渲染（DOMPurify 清洗，防 XSS）
import { computed } from 'vue'
import { marked } from 'marked'
import DOMPurify from 'dompurify'

const props = defineProps<{ content: string }>()

marked.setOptions({ breaks: true, gfm: true })

const html = computed(() => {
  const raw = marked.parse(props.content || '', { async: false }) as string
  return DOMPurify.sanitize(raw, { ALLOWED_TAGS: undefined, ADD_ATTR: ['target'] })
})
</script>

<template>
  <div class="md" v-html="html"></div>
</template>
