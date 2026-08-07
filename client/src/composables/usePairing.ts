import { computed, onScopeDispose, ref } from 'vue'

import type { PairingHandshake, PairingOffer, PairingResult } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Сопряжение устройств (F8, §2.2).
 *
 * Почему это composable, а не стор. Код сопряжения — не метаданные: тот, кто
 * его прочитал, получает право забрать копию хранилища. Обращаемся с ним как с
 * секретом (Закон №1): он живёт в области видимости экрана, не попадает ни в
 * Pinia, ни в localStorage, и исчезает при уходе с экрана и при блокировке
 * хранилища. Проверяется тестом.
 *
 * Разбором кода занимается ЯДРО: сюда приходит готовая матрица QR, отсюда
 * уходит прочитанная строка как есть. Фронт не знает формата пейлоада и не
 * должен узнать — иначе разбор ключей начал бы жить в JS.
 */

/** Как часто тикает обратный отсчёт срока жизни кода. */
const TICK_MS = 1000

/** `02:41` — как в макете (§3.6). */
export function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000))
  const minutes = Math.floor(total / 60)
  const seconds = total % 60
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`
}

/**
 * Разделитель кода для чтения глазами: `4TQ·9MB`. Сам код остаётся
 * каноническим — разбивка нужна только чтобы его было проще продиктовать.
 */
export function formatManualCode(code: string): string {
  const middle = Math.ceil(code.length / 2)
  return `${code.slice(0, middle)} · ${code.slice(middle)}`
}

/** Сторона, которая ПОКАЗЫВАЕТ код. */
export function usePairingOffer() {
  const offer = ref<PairingOffer | null>(null)
  const busy = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)

  /** Тикающее «сейчас» — только для обратного отсчёта. */
  const nowMs = ref(Date.now())
  let ticker: ReturnType<typeof setInterval> | null = null

  const remainingMs = computed(() => {
    if (offer.value === null) return 0
    return Math.max(0, Date.parse(offer.value.expires_at) - nowMs.value)
  })

  /**
   * Код показан, но уже недействителен. Убирать его с экрана самостоятельно не
   * нужно: пользователь должен понять, почему второе устройство его не берёт.
   */
  const isExpired = computed(() => offer.value !== null && remainingMs.value <= 0)

  const remainingLabel = computed(() => formatRemaining(remainingMs.value))

  function stopTicker(): void {
    if (ticker !== null) {
      clearInterval(ticker)
      ticker = null
    }
  }

  function startTicker(): void {
    stopTicker()
    nowMs.value = Date.now()
    ticker = setInterval(() => {
      nowMs.value = Date.now()
    }, TICK_MS)
  }

  /** Забыть код: экран закрыли, хранилище заперли. */
  function forget(): void {
    offer.value = null
    stopTicker()
  }

  /** Попросить у ядра код. Каждый вызов — новый код, прежний перестаёт годиться. */
  async function request(): Promise<boolean> {
    busy.value = true
    error.value = null
    try {
      offer.value = await useCore().getPairingPayload()
      startTicker()
      return true
    } catch (cause) {
      forget()
      error.value = isCoreError(cause) ? cause.message : 'Не удалось получить код сопряжения.'
      return false
    } finally {
      busy.value = false
    }
  }

  const unsubscribe = watchLock(forget)

  onScopeDispose(() => {
    forget()
    unsubscribe?.()
  })

  return { offer, busy, error, remainingMs, remainingLabel, isExpired, request, forget }
}

/** Сторона, которая ЧИТАЕТ код второго устройства. */
export function usePairingScan() {
  /**
   * Прочитанный код ждёт сверки слов. Самой строки кода здесь нет: она уходит
   * в ядро транзитом, аргументом `submit`, и в состоянии не оседает.
   */
  const handshake = ref<PairingHandshake | null>(null)
  const result = ref<PairingResult | null>(null)
  const busy = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)
  /** Код устарел — экрану есть что предложить: попросить новый на втором устройстве. */
  const isExpired = ref(false)

  function reset(): void {
    handshake.value = null
    result.value = null
    error.value = null
    isExpired.value = false
  }

  function fail(cause: unknown, fallback: string): void {
    isExpired.value = isCoreError(cause, 'PAIRING_EXPIRED')
    error.value = isCoreError(cause) ? cause.message : fallback
  }

  /**
   * Отдать ядру прочитанное. `payload` — строка как есть: полный пейлоад из QR
   * или код, набранный руками. Что это, разбирает ядро.
   */
  async function submit(payload: string): Promise<boolean> {
    if (payload.trim() === '') return false

    busy.value = true
    error.value = null
    isExpired.value = false
    try {
      handshake.value = await useCore().submitPairedKey(payload)
      return true
    } catch (cause) {
      handshake.value = null
      fail(cause, 'Не удалось прочитать код.')
      return false
    } finally {
      busy.value = false
    }
  }

  /** Человек сверил слова на двух экранах (§2.2) — записываем устройство. */
  async function confirm(): Promise<boolean> {
    const session = handshake.value
    if (session === null) return false

    busy.value = true
    error.value = null
    try {
      result.value = await useCore().confirmPairing(session.session_id)
      handshake.value = null
      return true
    } catch (cause) {
      // Сеанс в ядре уже закрыт — держать его на экране нельзя, иначе кнопка
      // «Слова совпадают» обещала бы то, чего больше нет.
      handshake.value = null
      fail(cause, 'Не удалось завершить сопряжение.')
      return false
    } finally {
      busy.value = false
    }
  }

  /** Слова не совпали или передумали. Ядро забывает ключ второго устройства. */
  async function cancel(): Promise<void> {
    const session = handshake.value
    reset()
    if (session === null) return

    try {
      await useCore().cancelPairing(session.session_id)
    } catch {
      /* сеанс всё равно закрыт для UI: держать его на экране незачем */
    }
  }

  const unsubscribe = watchLock(reset)

  onScopeDispose(() => {
    reset()
    unsubscribe?.()
  })

  return { handshake, result, busy, error, isExpired, submit, confirm, cancel, reset }
}

/**
 * Подписка на замок: заперли хранилище — на экране не должно остаться ни кода,
 * ни начатого сопряжения.
 */
function watchLock(onLocked: () => void): Unsubscribe | null {
  try {
    return useCore().on('locked', onLocked)
  } catch {
    /* ядро ещё не поднято — подписка не критична для рендера */
    return null
  }
}
