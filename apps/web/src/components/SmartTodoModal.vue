<script setup lang="ts">
// 智能速记确认弹窗（v1.1.2）：把一句话拆出的待办逐条核对（标题/日期/时间可改），
// 勾选后一键入库。拆解是「建议」，这里才是落库的最后一关——避免 AI 理解歪了直接写脏数据。
import { computed, ref } from 'vue'
import type { ExtractedTodo } from '@/api/types'
import { useTodosStore } from '@/stores/todos'
import { useAppStore } from '@/stores/app'
import { t } from '@/i18n'

const props = defineProps<{ items: ExtractedTodo[]; isAi: boolean; source: string }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'saved', count: number): void }>()

const todos = useTodosStore()
const app = useAppStore()

const rows = ref(
  props.items.map((x) => ({
    on: true,
    title: x.title,
    date: x.date,
    time: x.time || ''
  }))
)
const saving = ref(false)

const pickedCount = computed(() => rows.value.filter((r) => r.on && r.title.trim()).length)

function toggleRow(i: number) {
  rows.value[i].on = !rows.value[i].on
}

async function save() {
  const picked = rows.value.filter((r) => r.on && r.title.trim())
  if (!picked.length) return
  saving.value = true
  let ok = 0
  try {
    for (const r of picked) {
      await todos.create({
        title: r.title.trim().slice(0, 100),
        description: '',
        dueDate: r.date,
        dueTime: r.time || null,
        priority: '中',
        tags: [t('智能速记')]
      })
      ok++
    }
    app.toast('success', t('已添加 {a} 条待办', { a: ok }))
    emit('saved', ok)
  } catch (e: any) {
    app.toast('error', e?.message || t('保存失败'))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="modal-mask" @click.self="emit('close')">
    <div class="modal">
      <h3>{{ $t('拆为待办') }}</h3>
      <div class="small muted" style="margin-bottom: 10px">
        {{ isAi ? $t('AI 从这句话里拆出了下面的待办，可修改后入库') : $t('本地规则从这句话里拆出了待办，配置 AI 可更准') }}
        <span v-if="source" class="mono" :title="source"> · {{ source.slice(0, 24) }}{{ source.length > 24 ? '…' : '' }}</span>
      </div>

      <div class="extract-list">
        <label v-for="(r, i) in rows" :key="i" class="extract-row" :class="{ off: !r.on }">
          <input type="checkbox" :checked="r.on" @change="toggleRow(i)" />
          <input v-model="r.title" class="input extract-title" :placeholder="$t('标题')" maxlength="60" />
          <input v-model="r.date" type="date" class="input extract-date" />
          <input v-model="r.time" type="time" class="input extract-time" />
        </label>
      </div>
      <div v-if="!rows.length" class="empty" style="padding: 16px 0">
        <div class="t">{{ $t('这句话里没有拆出待办') }}</div>
        <div class="d">{{ $t('试着写清楚要做什么，或直接保存为一条记录') }}</div>
      </div>

      <div class="modal-actions">
        <button class="btn" @click="emit('close')">{{ $t('取消') }}</button>
        <button class="btn btn-primary" :disabled="saving || pickedCount === 0" @click="save">
          {{ saving ? $t('保存中…') : $t('添加选中的 {a} 条', { a: pickedCount }) }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.extract-list { display: flex; flex-direction: column; gap: 6px; max-height: 320px; overflow-y: auto; }
.extract-row { display: grid; grid-template-columns: auto 1fr 130px 100px; gap: 6px; align-items: center; }
.extract-row.off { opacity: 0.45; }
.extract-title { height: 30px; font-size: 13px; }
.extract-date, .extract-time { height: 30px; font-size: 12px; padding: 0 6px; }
</style>
