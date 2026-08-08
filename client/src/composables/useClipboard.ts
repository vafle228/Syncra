import { computed, onScopeDispose, ref } from 'vue'

/**
 * Копирование в буфер обмена с самоочисткой (F5, §4.5 макета).
 *
 * ЗАКОН №1: скопированное значение проходит здесь ТРАНЗИТОМ — уходит в
 * `writeText` и тут же отпускается. Ни в ref, ни в замыкании, ни в таймере
 * копии секрета не остаётся: таймер знает только то, что «пора чистить».
 *
 * Про очистку без сверки. Правильнее было бы перед очисткой убедиться, что в
 * буфере всё ещё лежит именно наш пароль. Но `readText` требует разрешения, а
 * сверка требует держать секрет всё это время — то есть ровно то, чего мы
 * избегаем. Поэтому чистим безусловно, а пользователю прямо говорим, что буфер
 * очистится: предсказуемое поведение лучше умного.
 *
 * Через сколько чистить, решает не этот файл: срок приходит аргументом
 * `clearAfterMs` от вызывающего, а тот берёт его из `securityPolicy()` —
 * настройки живут в ядре (F13). Здесь `0` по-прежнему означает «не чистить», и
 * на этом держится копирование метаданных: адрес сайта затирать незачем.
 */

function clipboardApi(): Clipboard | null {
  return globalThis.navigator?.clipboard ?? null
}

// ---------------------------------------------------------------------------
// Очистка живёт вне компонента
// ---------------------------------------------------------------------------

/**
 * Таймер очистки — модульный, а не привязанный к компоненту. Обещание «буфер
 * очистится через 20 с» не должно ломаться от того, что пользователь закрыл
 * карточку через две секунды после копирования.
 */
let clearTimer: ReturnType<typeof setTimeout> | null = null

/** Немедленно затереть буфер — например, когда хранилище заблокировалось. */
export async function clearClipboardNow(): Promise<void> {
  if (clearTimer !== null) {
    clearTimeout(clearTimer)
    clearTimer = null
  }
  try {
    await clipboardApi()?.writeText('')
  } catch {
    /* нет прав на запись — значит, и копирования не случилось */
  }
}

function scheduleClear(afterMs: number): void {
  if (clearTimer !== null) clearTimeout(clearTimer)
  clearTimer = setTimeout(() => {
    clearTimer = null
    void clearClipboardNow()
  }, afterMs)
}

// ---------------------------------------------------------------------------
// Composable
// ---------------------------------------------------------------------------

export interface CopyOptions {
  /** Через сколько чистить буфер. `0` — не чистить (для метаданных). */
  clearAfterMs?: number
}

/**
 * Состояние кнопок копирования одного экрана. `key` — что именно скопировали
 * («password», «login», конкретный адрес): по нему кнопка понимает, ей ли
 * показывать отсчёт.
 */
export function useClipboard() {
  const copiedKey = ref<string | null>(null)
  /** Сколько секунд осталось до очистки буфера. `0` — очистки не будет. */
  const secondsLeft = ref(0)
  const failed = ref(false)

  const available = computed(() => clipboardApi() !== null)

  let tick: ReturnType<typeof setInterval> | null = null

  function stopTick(): void {
    if (tick !== null) {
      clearInterval(tick)
      tick = null
    }
  }

  /** Забыть о копировании: кнопка возвращается в исходный вид. */
  function forget(): void {
    stopTick()
    copiedKey.value = null
    secondsLeft.value = 0
  }

  function startCountdown(afterMs: number): void {
    stopTick()
    secondsLeft.value = Math.ceil(afterMs / 1000)
    tick = setInterval(() => {
      secondsLeft.value -= 1
      if (secondsLeft.value <= 0) forget()
    }, 1000)
  }

  /**
   * Скопировать значение. Возвращает `true`, если получилось.
   * `text` здесь не сохраняется — сразу уходит в системный буфер.
   */
  async function copy(key: string, text: string, options: CopyOptions = {}): Promise<boolean> {
    const api = clipboardApi()
    if (!api) {
      failed.value = true
      return false
    }

    try {
      await api.writeText(text)
    } catch {
      failed.value = true
      forget()
      return false
    }

    failed.value = false
    copiedKey.value = key

    const clearAfterMs = options.clearAfterMs ?? 0
    if (clearAfterMs > 0) {
      scheduleClear(clearAfterMs)
      startCountdown(clearAfterMs)
    } else {
      stopTick()
      secondsLeft.value = 0
    }

    return true
  }

  onScopeDispose(stopTick)

  return { copiedKey, secondsLeft, failed, available, copy, forget }
}
