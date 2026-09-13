<script setup lang="ts">
// 登录页（局域网 / 服务器模式）
import { onMounted, ref } from 'vue'
import { useAppStore } from '@/stores/app'
import { api } from '@/api/client'

const app = useAppStore()
const password = ref('')
const confirm = ref('')
const loading = ref(false)
const needsSetup = ref(false)
const error = ref('')

onMounted(async () => {
  try {
    const s = await api.authStatus()
    app.auth = s
    needsSetup.value = !s.hasPassword
  } catch (e: any) {
    error.value = e?.message || '无法连接服务'
  }
})

async function submit() {
  error.value = ''
  if (needsSetup.value) {
    if (password.value.length < 6) {
      error.value = '密码至少 6 位'
      return
    }
    if (password.value !== confirm.value) {
      error.value = '两次输入的密码不一致'
      return
    }
  }
  loading.value = true
  try {
    if (needsSetup.value) {
      await api.setPassword(password.value)
      needsSetup.value = false
    }
    await app.login(password.value)
  } catch (e: any) {
    error.value = e?.message || '登录失败'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="login-wrap">
    <div class="login-card">
      <div class="logo-lg">M</div>
      <div class="center">
        <div style="font-size: 17px; font-weight: 600">{{ $t('智伴 Mindmate') }}</div>
        <div class="small muted">
          {{ needsSetup ? $t('首次使用，请设置访问密码') : $t('请输入访问密码') }}
        </div>
      </div>

      <div v-if="error" class="hint-bar warn">{{ error }}</div>

      <div class="form-row">
        <label class="form-label">{{ $t('访问密码') }}</label>
        <input
          v-model="password"
          type="password"
          class="input"
          :placeholder="$t('至少 6 位')"
          @keydown.enter="submit"
        />
      </div>

      <div v-if="needsSetup" class="form-row">
        <label class="form-label">{{ $t('确认密码') }}</label>
        <input v-model="confirm" type="password" class="input" @keydown.enter="submit" />
      </div>

      <button class="btn btn-primary" style="width: 100%; justify-content: center" :disabled="loading" @click="submit">
        {{ loading ? $t('处理中…') : needsSetup ? $t('设置并进入') : $t('登录') }}
      </button>

      <div class="small muted center">
        {{ $t('局域网/服务器模式下需要密码访问；本地模式无需登录') }}
      </div>
    </div>
  </div>
</template>
