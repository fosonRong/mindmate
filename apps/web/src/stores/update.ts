// 自动更新：检查 / 下载安装 / 重启 / 跳过此版本
//
// 只有桌面端可用；浏览器端所有 action 直接返回"不支持"。
// 更新包由 CI 用私钥签名，客户端凭 tauri.conf.json 里的公钥验签，签名不符会拒绝安装。
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { isDesktop } from '@/lib/desktop'
import { useAppStore } from '@/stores/app'

interface Available {
  version: string
  notes: string
  date?: string
}

export const useUpdateStore = defineStore('update', () => {
  const app = useAppStore()

  const checking = ref(false)
  const installing = ref(false)
  const available = ref<Available | null>(null)
  const progress = ref(0) // 0-100，未知总量时为 0
  const downloaded = ref(0)
  const total = ref(0)
  const error = ref('')
  const currentVersion = ref('')
  const lastCheckedAt = ref('')
  const upToDate = ref(false)

  // Update 实例（Tauri 的 Update 对象，不放进响应式）
  let pending: any = null

  const skippedVersion = computed(() => app.settings.skipped_version || '')
  const canUpdate = computed(() => isDesktop())
  const autoCheckEnabled = computed(() => app.settings.auto_update_check !== '0')

  /** 读取当前程序版本（Tauri app.getVersion） */
  async function loadVersion() {
    if (!isDesktop()) {
      currentVersion.value = '1.0.0'
      return
    }
    try {
      const { getVersion } = await import('@tauri-apps/api/app')
      currentVersion.value = await getVersion()
    } catch {
      /* 忽略 */
    }
  }

  /**
   * 检查更新
   * @param manual 手动点击时：即使已跳过该版本也要提示；自动检查时静默忽略已跳过版本
   */
  async function check(manual = false): Promise<void> {
    if (!isDesktop() || checking.value || installing.value) return
    checking.value = true
    error.value = ''
    upToDate.value = false
    try {
      const { check: tauriCheck } = await import('@tauri-apps/plugin-updater')
      const update = await tauriCheck()
      lastCheckedAt.value = new Date().toTimeString().slice(0, 5)
      if (!update) {
        upToDate.value = true
        if (manual) app.toast('success', '已是最新版本')
        return
      }
      if (!manual && skippedVersion.value && skippedVersion.value === update.version) {
        return // 用户跳过过的版本，自动检查不再打扰
      }
      pending = update
      available.value = {
        version: update.version,
        notes: (update.body || '').toString().slice(0, 500),
        date: update.date ? String(update.date) : undefined
      }
    } catch (e: any) {
      // 网络问题（例如无法访问静态清单）不应打断使用
      const msg = e?.message || String(e)
      error.value = msg
      if (manual) app.toast('error', `检查更新失败：${msg}`)
    } finally {
      checking.value = false
    }
  }

  /** 下载并安装，完成后重启应用 */
  async function install(): Promise<void> {
    if (!pending || installing.value) return
    installing.value = true
    error.value = ''
    try {
      await pending.downloadAndInstall((event: any) => {
        if (event.event === 'Started') {
          total.value = event.data?.contentLength || 0
        } else if (event.event === 'Progress') {
          downloaded.value += event.data?.chunkLength || 0
          progress.value = total.value ? Math.round((downloaded.value / total.value) * 100) : 0
        } else if (event.event === 'Finished') {
          progress.value = 100
        }
      })
      app.toast('success', '更新已安装，正在重启…')
      const { relaunch } = await import('@tauri-apps/plugin-process')
      await relaunch()
    } catch (e: any) {
      const msg = e?.message || String(e)
      error.value = msg
      app.toast('error', `更新失败：${msg}`)
    } finally {
      installing.value = false
    }
  }

  /** 跳过此版本（自动检查不再提示，手动检查仍可见） */
  async function skip(): Promise<void> {
    const v = available.value?.version
    if (!v) return
    await app.saveSettings({ skipped_version: v })
    available.value = null
    pending = null
    app.toast('info', `已跳过 v${v}，有新版本时仍会提示`)
  }

  function dismiss(): void {
    available.value = null
    pending = null
  }

  /** 启动后的自动检查（延迟执行，避免与首屏争资源） */
  function scheduleAutoCheck(delayMs = 30_000): void {
    if (!isDesktop() || !autoCheckEnabled.value) return
    setTimeout(() => {
      check(false)
    }, delayMs)
  }

  return {
    checking, installing, available, progress, downloaded, total, error,
    currentVersion, lastCheckedAt, upToDate, canUpdate, autoCheckEnabled, skippedVersion,
    loadVersion, check, install, skip, dismiss, scheduleAutoCheck
  }
})
