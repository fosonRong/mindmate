<script setup lang="ts">
// 待办编辑/新建弹窗
// v1.1.1：标签选项共用 stores/tags（内置 ∪ 自定义 ∪ 在用），可现场加词、✨ AI 打标；
// 新增 preset 预填（今日热点一键转待办用）。
import { computed, onMounted, ref } from 'vue'
import type { Todo } from '@/api/types'
import { useTodosStore } from '@/stores/todos'
import { useAppStore, todayStr, addDays } from '@/stores/app'
import { useTagsStore } from '@/stores/tags'
import { api } from '@/api/client'
import { t } from '@/i18n'

const props = defineProps<{
  todo?: Todo | null
  defaultDate?: string
  /** 预填（新建用）：标题/描述/标签，如热点新闻转待办 */
  preset?: { title?: string; description?: string; tags?: string[] } | null
}>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'saved'): void }>()

const todos = useTodosStore()
const app = useAppStore()
const tagsStore = useTagsStore()

const title = ref(props.todo?.title || props.preset?.title || '')
const description = ref(props.todo?.description || props.preset?.description || '')
const dueDate = ref(props.todo?.dueDate || props.defaultDate || todayStr())
// 有效期开始日期（v1.5.3）：[startDate, dueDate] 窗口内可做，超过截止才逾期。
// 新建默认今天（与截止一致，用户反馈）；编辑保持原值（旧单日待办不凭空注入开始日期）
const startDate = ref(props.todo ? (props.todo.startDate || '') : todayStr())
const dueTime = ref(props.todo?.dueTime || '')
const priority = ref(props.todo?.priority || '中')
const tags = ref<string[]>(
  props.todo?.tags ? [...props.todo.tags] : props.preset?.tags ? [...props.preset.tags] : []
)
const remindOffset = ref<number>(30)
const dueTime0 = computed(() => props.todo?.dueTime || '')
// 提醒模式（v1.2.2）：offset=按截止时间提前；at=指定时刻（remind_at 直传）；none=不提醒。
// 编辑时反推初始模式：有 remind_at → at；无 remind_at 但有截止时间 → offset；否则 none
const remindMode = ref<'offset' | 'at' | 'none'>(props.todo?.remindAt ? 'at' : props.todo ? (dueTime0.value ? 'offset' : 'none') : 'none')
const remindAtInput = ref(props.todo?.remindAt ? props.todo.remindAt.slice(11, 16) : '')
// 循环待办：''=不循环 daily=每天 weekly=每周 monthly=每月。
// 编辑既有循环实例时显示其类型；修改会写回链的根实例（后端按根重算后续生成）。
const recurType = ref<string>(props.todo?.recurType || '')
const recurUntil = ref<string>(props.todo?.recurUntil || '')
const recurInterval = ref<number>(props.todo?.recurInterval || 1)
const recurSkipRest = ref<boolean>(props.todo?.recurSkipRest === true)
const creating = ref(false)

const INTERVAL_UNIT: Record<string, string> = { daily: '天', weekly: '周', monthly: '月' }

const RECUR_OPTIONS = [
  { value: '', label: '不循环' },
  { value: 'daily', label: '每天' },
  { value: 'weekly', label: '每周' },
  { value: 'monthly', label: '每月' },
]

function toggleTag(t: string) {
  const i = tags.value.indexOf(t)
  if (i >= 0) tags.value.splice(i, 1)
  else tags.value.push(t)
}

// ── 新增自定义标签 / AI 打标（v1.1.1） ──
const addingTag = ref(false)
const newTag = ref('')
const tagSuggesting = ref(false)

async function confirmNewTag() {
  const name = newTag.value.trim()
  if (!name) {
    addingTag.value = false
    return
  }
  try {
    await tagsStore.addCustom(name)
    if (!tags.value.includes(name)) tags.value.push(name)
    newTag.value = ''
    addingTag.value = false
  } catch (e: any) {
    app.toast('error', e?.message || t('添加失败'))
  }
}

/** AI 从标题+描述里提取标签并选中 */
async function suggestTags() {
  const text = [title.value.trim(), description.value.trim()].filter(Boolean).join('\n')
  if (!text || tagSuggesting.value) return
  tagSuggesting.value = true
  try {
    const r = await api.aiTag(text)
    const fresh = r.tags.filter((x) => !tags.value.includes(x))
    if (fresh.length) {
      tags.value.push(...fresh)
      app.toast('success', t('AI 建议标签：{a}', { a: fresh.join('、') }))
    } else {
      app.toast('info', t('AI 没有想到更合适的标签'))
    }
  } catch (e: any) {
    app.toast('error', e?.message || t('AI 打标失败，请稍后再试'))
  } finally {
    tagSuggesting.value = false
  }
}

/** 按当前提醒模式算出 remind_at（v1.2.2）：at=指定时刻；offset=截止-提前量；none=null */
function resolvedRemindAt(): string | null {
  if (!dueDate.value) return null
  if (remindMode.value === 'at') {
    return remindAtInput.value ? `${dueDate.value} ${remindAtInput.value}` : null
  }
  if (remindMode.value === 'offset' && dueTime.value) {
    const [h, m] = dueTime.value.split(':').map(Number)
    const total = h * 60 + m - remindOffset.value
    const nh = Math.floor(total / 60) % 24
    const nm = ((total % 60) + 60) % 60
    const pad = (n: number) => String(n).padStart(2, '0')
    return `${dueDate.value} ${pad(nh)}:${pad(nm)}`
  }
  return null
}

async function save() {
  if (!title.value.trim()) {
    app.toast('warning', t('请填写待办标题'))
    return
  }
  creating.value = true
  try {
    if (props.todo) {
      await todos.update(props.todo.id, {
        title: title.value.trim(),
        description: description.value,
        dueDate: dueDate.value,
        startDate: startDate.value || '',
        dueTime: dueTime.value || null,
        priority: priority.value,
        tags: tags.value,
        remindAt: resolvedRemindAt(),
        recurType: recurType.value,
        recurUntil: recurUntil.value,
        recurInterval: recurInterval.value,
        recurSkipRest: recurSkipRest.value
      })
      app.toast('success', t('已保存'))
    } else {
      await todos.create({
        title: title.value.trim(),
        description: description.value,
        dueDate: dueDate.value,
        startDate: startDate.value || '',
        dueTime: dueTime.value || null,
        priority: priority.value,
        tags: tags.value,
        remindOffsetMin: dueTime.value && remindMode.value === 'offset' ? remindOffset.value : undefined,
        remindAt: resolvedRemindAt(),
        recurType: recurType.value,
        recurUntil: recurUntil.value,
        recurInterval: recurInterval.value,
        recurSkipRest: recurSkipRest.value
      })
      app.toast('success', t('已添加待办'))
    }
    emit('saved')
  } catch (e: any) {
    app.toast('error', e?.message || '保存失败')
  } finally {
    creating.value = false
  }
}

onMounted(() => tagsStore.load())
</script>

<template>
  <div class="modal-mask" @click.self="emit('close')">
    <div class="modal">
      <h3>{{ todo ? $t('编辑待办') : $t('新建待办') }}</h3>

      <div class="form-row">
        <label class="form-label">{{ $t('标题 *') }}</label>
        <input v-model="title" class="input" :placeholder="$t('要做什么？')" @keydown.enter="save" />
      </div>

      <div class="form-row">
        <label class="form-label">{{ $t('详细描述') }}</label>
        <textarea v-model="description" class="textarea" rows="2" :placeholder="$t('补充说明（可选）')"></textarea>
      </div>

      <div class="row" style="gap: 12px">
        <div class="form-row" style="flex: 1">
          <label class="form-label">{{ $t('开始日期（可选）') }}</label>
          <input v-model="startDate" type="date" class="input" />
        </div>
        <div class="form-row" style="flex: 1">
          <label class="form-label">{{ $t('截止日期') }}</label>
          <input v-model="dueDate" type="date" class="input" />
        </div>
      </div>
      <div class="row" style="gap: 12px">
        <div class="form-row" style="flex: 1">
          <label class="form-label">{{ $t('时间（填了即归入日程）') }}</label>
          <input v-model="dueTime" type="time" class="input" />
        </div>
        <div class="form-row" style="flex: 1"></div>
      </div>

      <!-- 重复独占一行：四个选项完整展示（此前与优先级挤一行，「不循环」被压成竖排） -->
      <div class="form-row">
        <label class="form-label">{{ $t('重复') }}</label>
        <div class="seg" style="width: 100%">
          <button
            v-for="o in RECUR_OPTIONS"
            :key="o.value"
            :class="{ on: recurType === o.value }"
            style="flex: 1; white-space: nowrap"
            @click="recurType = o.value"
          >
            {{ $t(o.label) }}
          </button>
        </div>
        <div class="small muted" style="margin-top: 4px">
          {{ recurType ? $t('完成后自动生成下一期（{a}）', { a: $t(RECUR_OPTIONS.find((o) => o.value === recurType)?.label || '') }) : $t('选择周期后，到期完成会自动生成下一期') }}
        </div>
        <template v-if="recurType">
          <div class="row" style="gap: 8px; margin-top: 6px">
            <span class="small muted" style="flex: none">{{ $t('每') }}</span>
            <select v-model.number="recurInterval" class="input" style="width: 72px; height: 30px">
              <option v-for="n in 6" :key="n" :value="n">{{ n }}</option>
            </select>
            <span class="small muted">{{ $t(INTERVAL_UNIT[recurType] || '') }}</span>
            <div class="spacer"></div>
            <label class="small muted" style="flex: none">{{ $t('结束日期') }}</label>
            <input v-model="recurUntil" type="date" class="input" style="width: 150px; height: 30px" />
          </div>
          <div class="row" style="margin-top: 6px">
            <span class="small muted" style="flex: 1">{{ $t('落在休息日（周末/法定休假）顺延到下一个工作日') }}</span>
            <div class="switch" :class="{ on: recurSkipRest }" @click="recurSkipRest = !recurSkipRest"></div>
          </div>
        </template>
      </div>

      <div class="row" style="gap: 12px">
        <div class="form-row" style="flex: 1">
          <label class="form-label">{{ $t('优先级') }}</label>
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
          <label class="form-label">{{ $t('提醒') }}</label>
          <div class="row" style="gap: 6px">
            <select v-if="remindMode === 'offset'" v-model.number="remindOffset" class="input" style="flex: 1">
              <option :value="5">{{ $t('提前 5 分钟') }}</option>
              <option :value="15">{{ $t('提前 15 分钟') }}</option>
              <option :value="30">{{ $t('提前 30 分钟') }}</option>
              <option :value="60">{{ $t('提前 1 小时') }}</option>
              <option :value="0">{{ $t('准点') }}</option>
            </select>
            <input
              v-else-if="remindMode === 'at'"
              v-model="remindAtInput"
              type="time"
              class="input"
              style="flex: 1"
            />
            <span v-else class="small muted" style="flex: 1">{{ $t('不提醒') }}</span>
            <select v-model="remindMode" class="input" style="width: 96px; flex: none">
              <option value="offset">{{ $t('提前') }}</option>
              <option value="at">{{ $t('指定时刻') }}</option>
              <option value="none">{{ $t('关闭') }}</option>
            </select>
          </div>
        </div>
        <div v-else class="form-row" style="flex: 1">
          <label class="form-label">{{ $t('提醒') }}</label>
          <div class="row" style="gap: 6px">
            <input
              v-if="remindMode === 'at'"
              v-model="remindAtInput"
              type="time"
              class="input"
              style="flex: 1"
            />
            <span v-else class="small muted" style="flex: 1">{{ $t('全天待办可指定时刻提醒（选「指定时刻」）') }}</span>
            <select v-model="remindMode" class="input" style="width: 96px; flex: none">
              <option value="none">{{ $t('关闭') }}</option>
              <option value="at">{{ $t('指定时刻') }}</option>
            </select>
          </div>
        </div>
      </div>

      <div class="form-row">
        <label class="form-label">
          {{ $t('标签') }}
          <button
            class="tag-pick ai-tag"
            style="margin-left: 8px"
            :disabled="tagSuggesting || !title.trim()"
            :title="app.aiReady ? $t('AI 从内容里提取标签') : $t('配置 AI 后可智能打标（当前按已有标签匹配）')"
            @click="suggestTags"
          >
            {{ tagSuggesting ? $t('思考中…') : $t('✨ AI 打标') }}
          </button>
        </label>
        <div class="row wrap">
          <button
            v-for="t in tagsStore.options"
            :key="t"
            class="tag-pick"
            :class="{ on: tags.includes(t) }"
            @click="toggleTag(t)"
          >
            {{ t }}
          </button>
          <input
            v-if="addingTag"
            v-model="newTag"
            class="input tag-add-input"
            :placeholder="$t('新标签，回车确认')"
            maxlength="12"
            @keydown.enter.prevent="confirmNewTag"
            @blur="confirmNewTag"
          />
          <button v-else class="tag-pick tag-add" :title="$t('新增自定义标签')" @click="addingTag = true">＋</button>
        </div>
      </div>

      <div class="row wrap" style="gap: 6px">
        <span class="small muted">{{ $t('快捷日期：') }}</span>
        <button class="btn btn-sm" @click="dueDate = todayStr()">{{ $t('今天') }}</button>
        <button class="btn btn-sm" @click="dueDate = addDays(todayStr(), 1)">{{ $t('明天') }}</button>
        <button class="btn btn-sm" @click="dueDate = addDays(todayStr(), 7)">{{ $t('下周') }}</button>
      </div>

      <div class="modal-actions">
        <button class="btn" @click="emit('close')">{{ $t('取消') }}</button>
        <button class="btn btn-primary" :disabled="creating" @click="save">
          {{ creating ? $t('保存中…') : $t('保存') }}
        </button>
      </div>
    </div>
  </div>
</template>
