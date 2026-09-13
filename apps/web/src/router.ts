import { createRouter, createWebHashHistory } from 'vue-router'

// 使用 hash 模式：兼容桌面端（file 协议 / 自定义协议）与浏览器端
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', name: 'today', component: () => import('@/views/Today.vue'), meta: { title: '今日', icon: 'home' } },
    { path: '/week', name: 'week', component: () => import('@/views/Week.vue'), meta: { title: '周', icon: 'calendar' } },
    { path: '/month', name: 'month', component: () => import('@/views/Month.vue'), meta: { title: '月', icon: 'grid' } },
    { path: '/todos', name: 'todos', component: () => import('@/views/Todos.vue'), meta: { title: '待办', icon: 'check' } },
    { path: '/companion', name: 'companion', component: () => import('@/views/Companion.vue'), meta: { title: '智伴', icon: 'spark' } },
    { path: '/settings', name: 'settings', component: () => import('@/views/Settings.vue'), meta: { title: '设置', icon: 'gear' } },
    { path: '/quick', name: 'quick', component: () => import('@/views/Quick.vue'), meta: { title: '速记', bare: true } },
    { path: '/login', name: 'login', component: () => import('@/views/Login.vue'), meta: { title: '登录', bare: true } },
    { path: '/:pathMatch(.*)*', redirect: '/' }
  ]
})
