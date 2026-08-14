import { onScopeDispose, ref, watch, type Ref } from 'vue'

import type { RecordId } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Живой код подтверждения одной записи (F5, карточка).
 *
 * ЗАКОН №1. Код считает ЯДРО (`get_totp_code`) — во фронте нет ни крипты, ни
 * `totp_secret`. Полученное число живёт по тем же правилам, что и остальные
 * секреты: в локальном `ref` этого composable, то есть в области видимости
 * компонента, а не в Pinia и не в localStorage. Оно исчезает при закрытии, при
 * смене записи, при размонтировании и по блокировке хранилища.
 *
 * Отдельное правило именно для кода: он живёт ОКНО. Когда окно кончается,
 * старый код становится неверным, и вместо него запрашивается следующий —
 * иначе на экране висело бы число, которое сервис уже не примет.
 */
export function useTotpCode(recordId: Ref<RecordId | null>) {
  /** Показанный код. `null` — закрыт (или его нет). */
  const code = ref<string | null>(null)
  /** Сколько секунд этот код ещё действителен. */
  const secondsLeft = ref(0)
  /** Длина окна — из ответа ядра, а не из константы во фронте. */
  const periodS = ref(30)
  const busy = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит. */
  const error = ref<string | null>(null)

  let timer: ReturnType<typeof setInterval> | null = null

  function stopTimer(): void {
    if (timer !== null) {
      clearInterval(timer)
      timer = null
    }
  }

  function hide(): void {
    stopTimer()
    code.value = null
    secondsLeft.value = 0
  }

  /** Разовый запрос к ядру. Возвращает `false`, если показывать нечего. */
  async function fetchCode(): Promise<boolean> {
    const id = recordId.value
    if (id === null) return false

    busy.value = true
    error.value = null
    try {
      const fresh = await useCore().getTotpCode(id)
      code.value = fresh.code
      secondsLeft.value = fresh.seconds_left
      periodS.value = fresh.period_s
      return true
    } catch (cause) {
      error.value = isCoreError(cause) ? cause.message : 'Не удалось получить код подтверждения.'
      hide()
      return false
    } finally {
      busy.value = false
    }
  }

  /** Показать код и держать его свежим, пока поле открыто. */
  async function reveal(): Promise<void> {
    if (!(await fetchCode())) return

    stopTimer()
    timer = setInterval(() => {
      secondsLeft.value -= 1
      // Окно закрылось: старое число уже не подойдёт — берём следующее.
      if (secondsLeft.value <= 0) void fetchCode()
    }, 1000)
  }

  async function toggle(): Promise<void> {
    if (code.value !== null) hide()
    else await reveal()
  }

  // Сменилась запись — чужой код на экране остаться не может.
  watch(recordId, () => {
    hide()
    error.value = null
  })

  let unsubscribe: Unsubscribe | null = null
  try {
    unsubscribe = useCore().on('locked', () => {
      hide()
    })
  } catch {
    /* ядро ещё не поднято — подписка не критична для рендера */
  }

  onScopeDispose(() => {
    hide()
    unsubscribe?.()
    unsubscribe = null
  })

  return { code, secondsLeft, periodS, busy, error, reveal, hide, toggle }
}
