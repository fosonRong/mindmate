/**
 * 桌面端能力封装（Tauri 2）
 * 浏览器端自动降级为空实现 —— 双端共用同一份前端代码。
 */

export function isDesktop(): boolean {
  return typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__
}

/** 调用 Rust 侧命令（浏览器端返回 null） */
export async function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!isDesktop()) return null
  try {
    const mod = await import('@tauri-apps/api/core')
    return (await mod.invoke<T>(cmd, args)) as T
  } catch (e) {
    console.warn(`[mindmate] invoke(${cmd}) 失败`, e)
    return null
  }
}

/** 打开速记浮窗（桌面端）。返回 true 表示桌面端已接管（后端命令返回布尔值，不再依赖非空判断） */
export async function openQuickEntry(): Promise<boolean> {
  return (await invoke<boolean>('open_quick_entry')) === true
}

/** 关闭速记浮窗（桌面端）——只隐藏窗口，不销毁 WebView */
export async function closeQuickEntry(): Promise<void> {
  await invoke('close_quick_entry')
}

/** 后端实际监听端口（桌面端由 Rust 注入） */
export function backendPort(): number {
  const p = (window as any).__MINDMATE_PORT__
  return typeof p === 'number' ? p : 17801
}
