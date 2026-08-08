import { onScopeDispose, reactive, ref, watch, type Ref } from 'vue'

import type { GetConflictSecretResponse, RecordId, SecretField } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

import { securityPolicy } from './securityPolicy'

/**
 * Секретные поля двух версий записи при разрешении конфликта (F11, §5.5).
 *
 * ЗАКОН №1 действует здесь ровно так же, как в `useRecordSecrets`: значения
 * приходят разово, по явному нажатию, живут в области видимости компонента и
 * исчезают по таймеру, при смене записи, при размонтировании и при блокировке
 * хранилища. Ни Pinia, ни localStorage.
 *
 * Почему отдельный composable, а не `useRecordSecrets`: там открывается ОДНО
 * значение записи, здесь — пара «местное / приехавшее». Разрешать конфликт,
 * глядя на один пароль, нельзя: вопрос ровно в том, чем они отличаются.
 *
 * Копирования тут нет намеренно. Секрет из конфликта нужен, чтобы сравнить
 * версии глазами; всё, что человек скопировал бы, он получит из карточки после
 * того, как выберет версию, — и уже с очисткой буфера.
 */

/** Одно поле в двух версиях. `null` — поле не заполнено в этой версии. */
export type ConflictSecretPair = GetConflictSecretResponse

type FieldMap<T> = Record<SecretField, T>

const FIELDS: SecretField[] = ['password', 'notes', 'totp_secret']

function emptyMap<T>(value: T): FieldMap<T> {
  return { password: value, notes: value, totp_secret: value }
}

export function useConflictSecrets(recordId: Ref<RecordId | null>) {
  /** Открытые прямо сейчас пары. `null` — поле закрыто. */
  const shown = reactive<FieldMap<ConflictSecretPair | null>>(
    emptyMap<ConflictSecretPair | null>(null),
  )
  /** Сколько секунд осталось до авто-скрытия. */
  const hideIn = reactive<FieldMap<number>>(emptyMap(0))
  const busy = ref<SecretField | null>(null)
  /** Сообщение ядра для показа пользователю. Секретов не содержит. */
  const error = ref<string | null>(null)

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
    hideIn[field] = Math.ceil(securityPolicy().value.secret_reveal_ms / 1000)
    timers[field] = setInterval(() => {
      hideIn[field] -= 1
      if (hideIn[field] <= 0) hide(field)
    }, 1000)
  }

  /** Открыть одно поле обеих версий. Закроется само через `secret_reveal_ms`. */
  async function reveal(field: SecretField): Promise<void> {
    const id = recordId.value
    if (id === null) return

    busy.value = field
    error.value = null
    try {
      shown[field] = await useCore().getConflictSecret(id, field)
      startAutoHide(field)
    } catch (cause) {
      error.value = isCoreError(cause) ? cause.message : 'Не удалось получить значения из ядра.'
    } finally {
      busy.value = null
    }
  }

  async function toggle(field: SecretField): Promise<void> {
    if (shown[field] !== null) hide(field)
    else await reveal(field)
  }

  watch(recordId, () => {
    hideAll()
    error.value = null
  })

  let unsubscribe: Unsubscribe | null = null
  try {
    unsubscribe = useCore().on('locked', hideAll)
  } catch {
    /* ядро ещё не поднято — подписка не критична для рендера */
  }

  onScopeDispose(() => {
    hideAll()
    unsubscribe?.()
    unsubscribe = null
  })

  return { shown, hideIn, busy, error, reveal, hide, hideAll, toggle }
}
