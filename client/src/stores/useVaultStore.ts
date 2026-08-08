import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import type {
  ChangeMasterPasswordResponse,
  IsoDateTime,
  PinStatus,
  RestoreBackupResult,
  UnlockWithPinResponse,
} from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Состояние замка хранилища (F3).
 *
 * ЗАКОН №1: мастер-пароль проходит через действия этого стора ТРАНЗИТОМ —
 * как аргумент, который уходит в ядро и тут же отпускается. В состоянии его нет
 * и быть не может; это проверяется тестом. Здесь же нет ни ключей, ни секретов
 * записей — только «создано / заблокировано / открыто».
 */

/**
 * Состояние замка глазами UI. Отличается от `VaultStatus` из контракта:
 * там сырой ответ ядра, здесь — то, чем управляется навигация,
 * включая «ещё не спрашивали».
 */
export type VaultLockStatus =
  /** Ещё не спрашивали ядро. */
  | 'unknown'
  /** Хранилища на устройстве нет — первый запуск, нужен онбординг. */
  | 'uninitialized'
  | 'locked'
  | 'unlocked'

/** Почему хранилище закрылось — для текста на экране входа. */
export type LockReason = 'manual' | 'timeout' | 'system' | null

export const useVaultStore = defineStore('vault', () => {
  const status = ref<VaultLockStatus>('unknown')
  const unlockedAt = ref<IsoDateTime | null>(null)
  const lockReason = ref<LockReason>(null)
  /**
   * Быстрый вход по PIN на ЭТОМ устройстве (F13). Не секрет: здесь только
   * «заведён ли» и «сколько попыток осталось» — сам PIN сюда не попадает
   * никогда, как и мастер-пароль.
   */
  const pin = ref<PinStatus>({ enrolled: false, attempts_left: null })
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)
  const busy = ref(false)

  const isUnlocked = computed(() => status.value === 'unlocked')
  const isReady = computed(() => status.value !== 'unknown')

  let unsubscribe: Unsubscribe | null = null
  let pendingStatus: Promise<void> | null = null

  /**
   * Подписка на события ядра: заблокировать нас может не только пользователь,
   * но и таймаут бездействия или сон системы.
   */
  function watchCore(): void {
    if (unsubscribe) return
    const core = useCore()

    const stopLocked = core.on('locked', (payload) => {
      status.value = 'locked'
      unlockedAt.value = null
      lockReason.value = payload.reason
    })

    const stopUnlocked = core.on('unlocked', (payload) => {
      status.value = 'unlocked'
      unlockedAt.value = payload.unlocked_at
      lockReason.value = null
    })

    unsubscribe = () => {
      stopLocked()
      stopUnlocked()
    }
  }

  function applyError(cause: unknown): never {
    error.value = isCoreError(cause) ? cause.message : 'Не удалось связаться с ядром.'
    throw cause
  }

  /** Спросить ядро о состоянии хранилища. */
  async function refresh(): Promise<VaultLockStatus> {
    watchCore()
    busy.value = true
    try {
      const vault = await useCore().getVaultStatus()
      status.value = !vault.initialized ? 'uninitialized' : vault.unlocked ? 'unlocked' : 'locked'
      unlockedAt.value = vault.unlocked_at
      pin.value = vault.pin
      return status.value
    } catch (cause) {
      // Не знаем состояния — считаем закрытым: это безопасная сторона ошибки.
      status.value = 'locked'
      return applyError(cause)
    } finally {
      busy.value = false
    }
  }

  /**
   * Гарантирует, что состояние известно. Дёргается навигационным хранителем
   * на каждом переходе, поэтому спрашивает ядро один раз и склеивает
   * параллельные вызовы в один запрос.
   */
  async function ensureStatus(): Promise<VaultLockStatus> {
    if (status.value !== 'unknown') return status.value
    pendingStatus ??= refresh()
      .then(() => undefined)
      .catch(() => undefined)
      .finally(() => {
        pendingStatus = null
      })
    await pendingStatus
    return status.value
  }

  /**
   * Разблокировать. `masterPassword` уходит в ядро и нигде не оседает —
   * ни в состоянии, ни в замыкании после возврата.
   */
  async function unlock(masterPassword: string): Promise<void> {
    watchCore()
    busy.value = true
    error.value = null
    try {
      const response = await useCore().unlock(masterPassword)
      status.value = 'unlocked'
      unlockedAt.value = response.unlocked_at
      lockReason.value = null
    } catch (cause) {
      applyError(cause)
    } finally {
      busy.value = false
    }
  }

  /**
   * Быстрый вход по PIN (F13).
   *
   * `pinInput` — транзитный аргумент ровно как мастер-пароль: уходит в ядро и
   * тут же отпускается, в состоянии его нет.
   *
   * Неверный PIN НЕ бросается и не пишется в `error`: это опечатка, а не сбой.
   * Возвращаем ответ ядра целиком, чтобы экран сам сказал, сколько попыток
   * осталось и не выключился ли быстрый вход совсем.
   */
  async function unlockByPin(pinInput: string): Promise<UnlockWithPinResponse> {
    watchCore()
    busy.value = true
    error.value = null
    try {
      const response = await useCore().unlockWithPin(pinInput)

      if (response.ok) {
        status.value = 'unlocked'
        unlockedAt.value = response.unlocked_at
        lockReason.value = null
        pin.value = { enrolled: true, attempts_left: null }
      } else {
        pin.value = response.pin_disabled
          ? { enrolled: false, attempts_left: null }
          : { enrolled: true, attempts_left: response.attempts_left }
      }

      return response
    } catch (cause) {
      applyError(cause)
    } finally {
      busy.value = false
    }
  }

  /**
   * Сменить мастер-пароль (F13). Оба пароля — транзитные аргументы.
   *
   * Хранилище остаётся открытым: старый пароль только что подтвердили.
   * Быстрый вход после перешифровки ядро сбрасывает — отражаем это здесь, а не
   * ждём следующего `refresh()`, иначе экран блокировки покажет клавиатуру,
   * которая больше ничего не отпирает.
   */
  async function changeMasterPassword(
    currentMasterPassword: string,
    newMasterPassword: string,
  ): Promise<ChangeMasterPasswordResponse> {
    watchCore()
    busy.value = true
    error.value = null
    try {
      const response = await useCore().changeMasterPassword(
        currentMasterPassword,
        newMasterPassword,
      )
      pin.value = { enrolled: false, attempts_left: null }
      return response
    } catch (cause) {
      applyError(cause)
    } finally {
      busy.value = false
    }
  }

  /** Создать хранилище (§3.9). Ключи генерирует ядро; сюда приходит только пароль. */
  async function initVault(masterPassword: string): Promise<void> {
    watchCore()
    busy.value = true
    error.value = null
    try {
      const response = await useCore().initVault(masterPassword)
      status.value = 'unlocked'
      unlockedAt.value = response.unlocked_at
      lockReason.value = null
    } catch (cause) {
      applyError(cause)
    } finally {
      busy.value = false
    }
  }

  /**
   * Восстановить хранилище из зашифрованного бэкапа (F12, §6.3).
   *
   * Резервный путь на случай «потеряли все устройства», поэтому он живёт рядом
   * с первым запуском: ядро принимает его только на устройстве без хранилища.
   * `null` — человек закрыл окно выбора файла; это не ошибка и показывать её
   * не надо. Пароль от файла проходит транзитом и в состоянии не остаётся.
   */
  async function restoreBackup(masterPassword: string): Promise<RestoreBackupResult | null> {
    watchCore()
    busy.value = true
    error.value = null
    try {
      const result = await useCore().restoreBackup(masterPassword)
      if (result === null) return null

      status.value = 'unlocked'
      unlockedAt.value = result.unlocked_at
      lockReason.value = null
      return result
    } catch (cause) {
      applyError(cause)
    } finally {
      busy.value = false
    }
  }

  async function lock(): Promise<void> {
    watchCore()
    busy.value = true
    error.value = null
    try {
      await useCore().lock()
      status.value = 'locked'
      unlockedAt.value = null
      lockReason.value = 'manual'
    } catch (cause) {
      applyError(cause)
    } finally {
      busy.value = false
    }
  }

  function clearError(): void {
    error.value = null
  }

  /** Снять подписку на события ядра — нужно тестам и горячей перезагрузке. */
  function dispose(): void {
    unsubscribe?.()
    unsubscribe = null
  }

  return {
    status,
    unlockedAt,
    lockReason,
    pin,
    error,
    busy,
    isUnlocked,
    isReady,
    refresh,
    ensureStatus,
    unlock,
    unlockByPin,
    changeMasterPassword,
    initVault,
    restoreBackup,
    lock,
    clearError,
    dispose,
  }
})
