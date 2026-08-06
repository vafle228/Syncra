import { defineStore } from 'pinia'
import { ref } from 'vue'

/**
 * Тосты (F2).
 *
 * ЗАКОН №1: текст тоста уходит в DOM и живёт секунды после действия —
 * секретов в нём быть не может. Правильно: «Пароль скопирован · буфер
 * очистится через 20 с». Неправильно: сам пароль, логин из чужой записи,
 * путь к файлу хранилища, текст ошибки ядра с внутренними деталями.
 */

export type ToastTone = 'neutral' | 'success' | 'warning' | 'danger'

export interface Toast {
  id: number
  text: string
  tone: ToastTone
}

/** Из макета: тост живёт 2.8 с и уходит сам. */
export const TOAST_TIMEOUT_MS = 2800

export const useToastStore = defineStore('toast', () => {
  const toasts = ref<Toast[]>([])
  const timers = new Map<number, ReturnType<typeof setTimeout>>()
  let nextId = 1

  function dismiss(id: number): void {
    const timer = timers.get(id)
    if (timer !== undefined) {
      clearTimeout(timer)
      timers.delete(id)
    }
    toasts.value = toasts.value.filter((toast) => toast.id !== id)
  }

  function push(text: string, tone: ToastTone = 'neutral', timeoutMs = TOAST_TIMEOUT_MS): number {
    const id = nextId++
    toasts.value = [...toasts.value, { id, text, tone }]

    if (timeoutMs > 0) {
      timers.set(
        id,
        setTimeout(() => dismiss(id), timeoutMs),
      )
    }

    return id
  }

  function clear(): void {
    for (const timer of timers.values()) clearTimeout(timer)
    timers.clear()
    toasts.value = []
  }

  return { toasts, push, dismiss, clear }
})
