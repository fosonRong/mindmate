<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAppStore, todayStr } from '@/stores/app'
import { useTodosStore } from '@/stores/todos'
import { useNodesStore } from '@/stores/nodes'
import { requestNotificationPermission } from '@/stores/app'
import { openQuickEntry as desktopQuickEntry, isDesktop } from '@/lib/desktop'
import Icon from '@/components/Icon.vue'
import Onboarding from '@/components/Onboarding.vue'
import AchievementsDrawer from '@/components/AchievementsDrawer.vue'
import UpdateBanner from '@/components/UpdateBanner.vue'
import { useUpdateStore } from '@/stores/update'

const app = useAppStore()
const todos = useTodosStore()
const nodes = useNodesStore()
const update = useUpdateStore()
const route = useRoute()
const router = useRouter()

const isBare = computed(() => route.meta.bare === true || !app.loggedIn)
const pageTitle = computed(() => (route.meta.title as string) || '智伴')
const dateLabel = computed(() => {
  const d = new Date()
  const week = ['日', '一', '二', '三', '四', '五', '六'][d.getDay()]
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')} · 周${week}`
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
  return m === 'local' ? '本地模式' : m === 'lan' ? '局域网模式' : '服务器模式'
})

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
      <router-link v-for="n in navItems" :key="n.to" :to="n.to" class="nav-item" :title="n.title">
        <Icon :name="n.icon" :size="20" />
      </router-link>
      <div class="nav-spacer"></div>
      <router-link to="/settings" class="nav-item" title="设置"><Icon name="gear" :size="20" /></router-link>
      <button
        v-if="app.stats && app.stats.streakDays > 0"
        class="streak-chip"
        title="连续记录天数 · 点击查看我的徽章"
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
          <div class="date">{{ dateLabel }}</div>
        </div>
        <div class="spacer"></div>
        <button class="btn btn-sm" @click="openQuickEntry" title="速记（Alt+Z）">
          ⚡ 速记 <span class="muted mono" style="font-size: 11px">Alt+Z</span>
        </button>
        <router-link to="/settings" class="icon-btn" title="设置"><Icon name="gear" :size="18" /></router-link>
      </header>

      <main class="page-body">
        <router-view />
      </main>

      <footer class="status-bar">
        <span>
          <i class="dot" :class="app.connected ? 'on' : 'off'"></i>
          {{ app.connected ? '已同步' : '连接中…' }}
        </span>
        <span>{{ modeLabel }}</span>
        <span v-if="app.stats">今日 {{ app.stats.nodeCount }} 条 · 待办 {{ app.stats.todayDoneTodos }}/{{ app.stats.todayTodos }}</span>
        <div class="spacer"></div>
        <span v-if="app.stats && app.stats.streakDays > 0">🔥 连续记录 {{ app.stats.streakDays }} 天</span>
        <span>v1.0.0</span>
      </footer>
    </div>

    <!-- 首次启动引导 -->
    <Onboarding v-if="showOnboarding" @done="showOnboarding = false" />

    <!-- 成就抽屉 -->
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
        <button v-if="r.action === 'quick_entry'" class="btn btn-sm btn-primary" @click="openQuickEntry(); app.dismissReminder(r.id)">速记</button>
        <button v-else class="btn btn-sm" @click="router.push(r.action === 'open_todos' ? '/todos' : '/'); app.dismissReminder(r.id)">查看</button>
        <button class="icon-btn" style="width: 22px; height: 22px" @click="app.dismissReminder(r.id)">×</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
</style>
