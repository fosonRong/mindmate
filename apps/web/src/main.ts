import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { router } from './router'
import { i18n, bootstrapLocale, safeT, t } from './i18n'
import { useAppStore } from './stores/app'
import './styles/base.css'
import './styles/app.css'

bootstrapLocale()

const app = createApp(App)
const pinia = createPinia()
app.use(pinia)
app.use(router)
app.use(i18n)

// 模板里的 $t 也走「永不抛异常」的 safeT：vue-i18n 全局注入的 $t 在词条语法非法时会抛
// SyntaxError，一旦发生在渲染中就会让整块界面变空白。问题词条由 scripts/i18n_check.mjs
// 在构建前拦截，这里的兜底保证线上永远只是「显示原文」而不是白屏。
app.config.globalProperties.$t = safeT

// 兜底之外的最后一道网：任何未捕获的组件异常都给出可见提示（5 秒内同一错误只提示一次）
const seen = new Map<string, number>()
app.config.errorHandler = (err, _instance, info) => {
  const msg = (err as Error)?.message || String(err)
  console.error(`[智伴] 组件异常（${info}）：`, err)
  const now = Date.now()
  if ((seen.get(msg) || 0) > now) return
  seen.set(msg, now + 5000)
  try {
    useAppStore(pinia).toast('error', t('界面出错：{a}', { a: msg }))
  } catch {
    /* store 尚不可用时仅记录控制台 */
  }
}

app.mount('#app')
