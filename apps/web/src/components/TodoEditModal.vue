<script setup lang="ts">
// 待办编辑/新建弹窗
import { ref, onMounted } from 'vue'
import type { Todo } from '@/api/types'
import { useTodosStore } from '@/stores/todos'
import { useAppStore, todayStr, addDays } from '@/stores/app'

const props = defineProps<{ todo?: Todo | null; defaultDate?: string }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'saved'): void }>()

const todos = useTodosStore()
const app = useAppStore()

const title = ref(props.todo?.title || '')
const description = ref(props.todo?.description || '')
const dueDate = ref(props.todo?.dueDate || props.defaultDate || todayStr())
const dueTime = ref(props.todo?.dueTime || '')
const priority = ref(props.todo?.priority || '中')
const tags = ref<string[]>(props.todo?.tags ? [...props.todo.tags] : [])
const remindOffset = ref<number>(30)
const creating = ref(false)

const TAG_OPTIONS = ['工作', '生活', '健康', '学习']

function toggleTag(t: string) {
  const i = tags.value.indexOf(t)
  if (i >= 0) tags.value.splice(i, 1)
  else tags.value.push(t)
}

async function save() {
  if (!title.value.trim()) {
    app.toast('warning', '请填写待办标题')
    return
  }
  creating.value = true
  try {
    if (props.todo) {
      await todos.update(props.todo.id, {
        title: title.value.trim(),
        description: description.value,
        dueDate: dueDate.value,
        dueTime: dueTime.value || null,
        priority: priority.value,
        tags: tags.value
      })
      app.toast('success', '已保存')
    } else {
      await todos.create({
        title: title.value.trim(),
        description: description.value,
        dueDate: dueDate.value,
        dueTime: dueTime.value || null,
        priority: priority.value,
        tags: tags.value,
        remindOffsetMin: dueTime.value ? remindOffset.value : undefined
      })
      app.toast('success', '已添加待办')
    }
    emit('saved')
  } catch (e: any) {
    app.toast('error', e?.message || '保存失败')
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <div class="modal-mask" @click.self="emit('close')">
    <div class="modal">
      <h3>{{ todo ? $t('编辑待办') : $t('新建待办') }}</h3>

      <div class="form-row">
        <label class="form-label">{{ $t($t('标题 *')) }}</label>
        <input v-model="title" class="input" :placeholder="$t('要做什么？')" @keydown.enter="save" />
      </div>

      <div class="form-row">
        <label class="form-label">{{ $t($t('详细描述')) }}</label>
        <textarea v-model="description" class="textarea" rows="2" :placeholder="$t('补充说明（可选）')"></textarea>
      </div>

      <div class="row" style="gap: 12px">
        <div class="form-row" style="flex: 1">
          <label class="form-label">{{ $t($t('截止日期')) }}</label>
          <input v-model="dueDate" type="date" class="input" />
        </div>
        <div class="form-row" style="flex: 1">
          <label class="form-label">{{ $t($t('时间（填了即归入日程）')) }}</label>
          <input v-model="dueTime" type="time" class="input" />
        </div>
      </div>

      <div class="row" style="gap: 12px">
        <div class="form-row" style="flex: 1">
          <label class="form-label">{{ $t($t('优先级')) }}</label>
          <div class="seg" style="width: 100%">
            <button
              v-for="p in ['高', '中', '低']"
              :key="p"
              :class="{ on: priority === p }"
              style="flex: 1"
              @click="priority = p"
            >
              {{ p }}
            </button>
          </div>
        </div>
        <div v-if="dueTime" class="form-row" style="flex: 1">
          <label class="form-label">{{ $t($t('提前提醒')) }}</label>
          <select v-model.number="remindOffset" class="input">
            <option :value="5">{{ $t($t('5 分钟')) }}</option>
            <option :value="15">{{ $t($t('15 分钟')) }}</option>
            <option :value="30">{{ $t($t('30 分钟')) }}</option>
            <option :value="60">{{ $t($t('1 小时')) }}</option>
            <option :value="0">{{ $t($t('准点')) }}</option>
          </select>
        </div>
      </div>

      <div class="form-row">
        <label class="form-label">{{ $t($t('标签')) }}</label>
        <div class="row wrap">
          <button
            v-for="t in TAG_OPTIONS"
            :key="t"
            class="tag-pick"
            :class="{ on: tags.includes(t) }"
            @click="toggleTag(t)"
          >
            {{ t }}
          </button>
        </div>
      </div>

      <div class="row wrap" style="gap: 6px">
        <span class="small muted">{{ $t($t('快捷日期：')) }}</span>
        <button class="btn btn-sm" @click="dueDate = todayStr()">{{ $t($t('今天')) }}</button>
        <button class="btn btn-sm" @click="dueDate = addDays(todayStr(), 1)">{{ $t($t('明天')) }}</button>
        <button class="btn btn-sm" @click="dueDate = addDays(todayStr(), 7)">{{ $t($t('下周')) }}</button>
      </div>

      <div class="modal-actions">
        <button class="btn" @click="emit('close')">{{ $t($t('取消')) }}</button>
        <button class="btn btn-primary" :disabled="creating" @click="save">
          {{ creating ? $t('保存中…') : $t('保存') }}
        </button>
      </div>
    </div>
  </div>
</template>
