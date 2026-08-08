import { onScopeDispose, reactive, ref, watch, type Ref } from 'vue'

import type { RecordId, SecretField } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

import { securityPolicy } from './securityPolicy'
import { clearClipboardNow, useClipboard } from './useClipboard'

/**
 * Доступ к секретам одной записи (F5).
 *
 * ЗАКОН №1 в чистом виде. Правила, которым подчинён каждый метод ниже:
 *  - секрет запрашивается РАЗОВО, по явному действию пользователя;
 *  - из ответа берётся ровно одно поле, остальные не сохраняются нигде;
 *  - открытое значение живёт в локальном `reactive` этого composable — то есть
 *    в области видимости компонента, а не в Pinia, не в localStorage и не в
 *    модульной переменной; оно исчезает по таймеру, при смене записи,
 *    при размонтировании и при блокировке хранилища;
 *  - копирование вообще не требует показа: значение уходит в буфер, не побывав
 *    на экране.
 *
 * Сроки авто-скрытия и очистки буфера приходят из настроек безопасности
 * (F13, `securityPolicy()`), а не из константы: пользователь выбирает их сам, и
 * два разных числа — на экране и в таймере — были бы обманом.
 */

type SecretMap<T> = Record<SecretField, T>

const FIELDS: SecretField[] = ['password', 'notes', 'totp_secret']

function emptyMap<T>(value: T): SecretMap<T> {
  return { password: value, notes: value, totp_secret: value }
}

export function useRecordSecrets(recordId: Ref<RecordId | null>) {
  /** Открытые прямо сейчас значения. `null` — поле закрыто. */
  const shown = reactive<SecretMap<string | null>>(emptyMap<string | null>(null))
  /** Сколько секунд осталось до авто-скрытия. */
  const hideIn = reactive<SecretMap<number>>(emptyMap(0))
  /** Какое поле сейчас едет через IPC — чтобы кнопка не дёргалась дважды. */
  const busy = ref<SecretField | null>(null)
  /** Сообщение ядра для показа пользователю. Секретов не содержит. */
  const error = ref<string | null>(null)

  const clipboard = useClipboard()

  const timers = emptyMap<ReturnType<typeof setInterval> | null>(null)

  function stopTimer(field: SecretField): void {
    const timer = timers[field]
    if (timer !== null) {
      clearInterval(timer)
      timers[field] = null
    }
  }

  function hide(field: SecretField): void {
    stopTimer(field)
    shown[field] = null
    hideIn[field] = 0
  }

  function hideAll(): void {
    for (const field of FIELDS) hide(field)
  }

  function startAutoHide(field: SecretField): void {
    stopTimer(field)
    // Срок читается В МОМЕНТ показа: настройку могли поменять между открытием
    // экрана и нажатием. Уже идущий отсчёт при этом не продлевается — открытый
    // секрет закрывается по тому сроку, который был обещан при открытии.
    hideIn[field] = Math.ceil(securityPolicy().value.secret_reveal_ms / 1000)
    timers[field] = setInterval(() => {
      hideIn[field] -= 1
      if (hideIn[field] <= 0) hide(field)
    }, 1000)
  }

  /**
   * Разовый запрос к ядру. Возвращает ровно одно поле; объект с остальными
   * секретами не покидает эту функцию.
   */
  async function fetchField(field: SecretField): Promise<string | null> {
    const id = recordId.value
    if (id === null) return null

    busy.value = field
    error.value = null
    try {
      const secrets = await useCore().getSecret(id)
      return secrets[field]
    } catch (cause) {
      error.value = isCoreError(cause) ? cause.message : 'Не удалось получить секрет из ядра.'
      return null
    } finally {
      busy.value = null
    }
  }

  /** Показать поле на экране. Закроется само через `secret_reveal_ms`. */
  async function reveal(field: SecretField): Promise<void> {
    const value = await fetchField(field)
    if (value === null) return

    shown[field] = value
    startAutoHide(field)
  }

  async function toggle(field: SecretField): Promise<void> {
    if (shown[field] !== null) hide(field)
    else await reveal(field)
  }

  /**
   * Скопировать поле, не показывая его. Значение не задерживается ни в одном
   * ref — только аргументом уходит в буфер обмена.
   */
  async function copy(field: SecretField): Promise<boolean> {
    const value = await fetchField(field)
    if (value === null || value === '') return false

    return clipboard.copy(field, value, {
      clearAfterMs: securityPolicy().value.clipboard_clear_ms,
    })
  }

  // Сменилась запись — открытое значение предыдущей закрываем немедленно.
  watch(recordId, () => {
    hideAll()
    clipboard.forget()
    error.value = null
  })

  /**
   * Блокировка хранилища закрывает и то, что уже на экране, и буфер обмена:
   * пользователь ушёл от компьютера, а пароль остался бы висеть.
   */
  let unsubscribe: Unsubscribe | null = null
  try {
    unsubscribe = useCore().on('locked', () => {
      hideAll()
      clipboard.forget()
      void clearClipboardNow()
    })
  } catch {
    /* ядро ещё не поднято — подписка не критична для рендера */
  }

  onScopeDispose(() => {
    hideAll()
    unsubscribe?.()
    unsubscribe = null
  })

  return {
    shown,
    hideIn,
    busy,
    error,
    clipboard,
    reveal,
    hide,
    hideAll,
    toggle,
    copy,
  }
}
