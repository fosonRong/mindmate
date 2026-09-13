import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { router } from './router'
import { i18n, bootstrapLocale } from './i18n'
import './styles/base.css'
import './styles/app.css'

bootstrapLocale()

const app = createApp(App)
app.use(createPinia())
app.use(router)
app.use(i18n)
app.mount('#app')
