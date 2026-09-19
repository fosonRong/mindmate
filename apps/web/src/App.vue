<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAppStore, todayStr } from '@/stores/app'
import { useTodosStore } from '@/stores/todos'
import { useNodesStore } from '@/stores/nodes'
import { requestNotificationPermission } from '@/stores/app'
import { openQuickEntry as desktopQuickEntry, isDesktop } from '@/lib/desktop'
import Icon from '@/components/Icon.vue'
import { daySubLabel, dayBadge, holidayInfo } from '@/lib/lunar'
import Onboarding from '@/components/Onboarding.vue'
import AchievementsDrawer from '@/components/AchievementsDrawer.vue'
import CommandPalette from '@/components/CommandPalette.vue'
import UpdateBanner from '@/components/UpdateBanner.vue'
import SafeView from '@/components/SafeView.vue'
import { useUpdateStore } from '@/stores/update'
import { t } from '@/i18n'

const app = useAppStore()
const todos = useTodosStore()
const nodes = useNodesStore()
const update = useUpdateStore()
const route = useRoute()
const router = useRouter()

const isBare = computed(() => route.meta.bare === true || !app.loggedIn)
const pageTitle = computed(() => t((route.meta.title as string) || '智伴'))
const today = (() => {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
})()
const todayLunar = daySubLabel(today)
const todayBadge = dayBadge(today)
const todayHolidayName = holidayInfo(today).name
const dateLabel = computed(() => {
  const d = new Date()
  const week = ['日', '一', '二', '三', '四', '五', '六'][d.getDay()]
  const day = today
  return `${day} · ${t('周' + week)}${todayLunar ? ' · ' + todayLunar : ''}`
})

const showOnboarding = ref(false)
const showAchievements = ref(false)

const navItems = [
  { to: '/', title: '今日', icon: 'home' },
  { to: '/week', title: '周', icon: 'calendar' },
  { to: '/month', title: '月', icon: 'grid' },
  { to: '/todos', title: '待办', icon: 'check' },
  { to: '/companion', title: '智伴', icon: 'spark' }
]

const modeLabel = computed(() => {
  const m = app.auth?.mode || 'local'
  return t(m === 'local' ? '本地模式' : m === 'lan' ? '局域网模式' : '服务器模式')
})

/** 呼出全局命令面板（Ctrl+K / 顶栏搜索按钮） */
function openPalette() {
  window.dispatchEvent(new CustomEvent('mindmate:open-palette'))
}

async function openQuickEntry() {
  // 桌面端：打开独立速记窗口；浏览器端：聚焦今日页速记框
  if (isDesktop()) {
    const ok = await desktopQuickEntry()
    if (ok) return
  }
  if (route.path !== '/') await router.push('/')
  window.dispatchEvent(new CustomEvent('mindmate:focus-quick-entry'))
}

function onKeydown(e: KeyboardEvent) {
  // 桌面端由全局热键处理；浏览器端提供 Alt+Z / 按 "/" 聚焦
  if (e.altKey && (e.key === 'z' || e.key === 'Z')) {
    e.preventDefault()
    openQuickEntry()
  }
}

/** 全局事件 → store 分发（跨端同步） */
function onGlobalEvent(e: Event) {
  const detail = (e as CustomEvent).detail
  nodes.onEvent(detail)
  todos.onEvent(detail)
  if (detail.kind === 'reminder.triggered') {
    const action = detail.payload?.action
    if (action === 'open_today' && route.path !== '/') router.push('/')
    if (action === 'open_todos' && route.path !== '/todos') router.push('/todos')
  }
}

onMounted(async () => {
  app.watchSystemTheme()
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('mindmate:event', onGlobalEvent)
  await app.init()
  if (app.loggedIn) {
    await Promise.all([nodes.load(), todos.load()])
    requestNotificationPermission()
    // 首次启动引导（里程碑 M7）
    if (app.settings.onboarded !== '1') showOnboarding.value = true
    // 自动更新：读取当前版本，并延迟做一次检查（可在设置中关闭）
    update.loadVersion()
    update.scheduleAutoCheck()
  }
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('mindmate:event', onGlobalEvent)
})
</script>

<template>
  <!-- 登录页（局域网/服务器模式） -->
  <router-view v-if="!app.loggedIn" />

  <!-- 速记浮窗（独立窗口，无壳） -->
  <router-view v-else-if="isBare" />

  <!-- 主应用 -->
  <div v-else class="app-shell">
    <nav class="side-nav">
      <div class="logo">M</div>
      <router-link v-for="n in navItems" :key="n.to" :to="n.to" class="nav-item" :title="t(n.title)">
        <Icon :name="n.icon" :size="20" />
      </router-link>
      <div class="nav-spacer"></div>
      <router-link to="/settings" class="nav-item" :title="$t('设置')"><Icon name="gear" :size="20" /></router-link>
      <button
        v-if="app.stats && app.stats.streakDays > 0"
        class="streak-chip"
        :title="$t('连续记录天数 · 点击查看我的徽章')"
        style="background: none; border: none; cursor: pointer"
        @click="showAchievements = true"
      >
        🔥{{ app.stats.streakDays }}
      </button>
    </nav>

    <div class="main">
      <header class="top-bar">
        <div>
          <h1>{{ pageTitle }}</h1>
          <div class="date">
            {{ dateLabel }}
            <span v-if="todayBadge" class="day-badge" :class="todayBadge === '休' ? 'off' : 'work'">{{ todayBadge }}</span>
            <span v-if="todayHolidayName" class="muted" style="margin-left: 4px">{{ todayHolidayName }}</span>
          </div>
        </div>
        <div class="spacer"></div>
        <button class="btn btn-sm" @click="openQuickEntry" :title="$t('速记（Alt+Z）')">
          {{ $t('⚡ 速记') }} <span class="muted mono" style="font-size: 11px">Alt+Z</span>
        </button>
        <button class="btn btn-sm" :title="$t('全局搜索与命令（Ctrl+K）')" @click="openPalette">
          🔍 <span class="muted mono" style="font-size: 11px">Ctrl+K</span>
        </button>
        <router-link to="/settings" class="icon-btn" :title="$t('设置')"><Icon name="gear" :size="18" /></router-link>
      </header>

      <main class="page-body">
        <SafeView>
          <router-view />
        </SafeView>
      </main>

      <footer class="status-bar">
        <span>
          <i class="dot" :class="app.connected ? 'on' : 'off'"></i>
          {{ app.connected ? $t('已同步') : $t('连接中…') }}
        </span>
        <span>{{ modeLabel }}</span>
        <span v-if="app.stats">{{ $t('今日 {a} 条 · 待办 {b}/{c}', { a: app.stats.nodeCount, b: app.stats.todayDoneTodos, c: app.stats.todayTodos }) }}</span>
        <div class="spacer"></div>
        <span v-if="app.stats && app.stats.streakDays > 0">{{ $t('🔥 连续记录 {a} 天', { a: app.stats.streakDays }) }}</span>
        <span v-if="update.currentVersion">v{{ update.currentVersion }}</span>
      </footer>
    </div>

    <!-- 首次启动引导 -->
    <Onboarding v-if="showOnboarding" @done="showOnboarding = false" />

    <!-- 成就抽屉 -->
    <CommandPalette />
      <AchievementsDrawer v-if="showAchievements" @close="showAchievements = false" />

    <!-- 新版本提示（桌面端） -->
    <UpdateBanner v-if="app.loggedIn" />

    <!-- Toast -->
    <div class="toast-wrap">
      <div v-for="t in app.toasts" :key="t.id" class="toast-item" :class="t.kind">{{ t.text }}</div>
    </div>

    <!-- 提醒横幅 -->
    <div class="reminder-wrap">
      <div v-for="r in app.reminders" :key="r.id" class="reminder-item" :class="r.kind">
        <div class="icon">{{ r.kind === 'overdue' ? '⚠️' : r.kind === 'care' ? '💬' : r.kind === 'todo' ? '📌' : '🕘' }}</div>
        <div style="flex: 1; min-width: 0">
          <div style="font-size: 13px; font-weight: 600">{{ r.title }}</div>
          <div class="small muted">{{ r.body }}</div>
        </div>
        <button v-if="r.action === 'quick_entry'" class="btn btn-sm btn-primary" @click="openQuickEntry(); app.dismissReminder(r.id)">{{ $t('速记') }}</button>
        <button v-else class="btn btn-sm" @click="router.push(r.action === 'open_todos' ? '/todos' : '/'); app.dismissReminder(r.id)">{{ $t('查看') }}</button>
        <button class="icon-btn" style="width: 22px; height: 22px" @click="app.dismissReminder(r.id)">×</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
</style>
