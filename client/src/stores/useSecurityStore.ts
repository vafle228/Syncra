import { defineStore } from 'pinia'
import { ref } from 'vue'

import { resetSecurityPolicy, setSecurityPolicy } from '@/composables/securityPolicy'
import type { SecuritySettings, SecuritySettingsPatch } from '@/core/contract'
import { DEFAULT_SECURITY_SETTINGS } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Настройки безопасности: таймауты замка, буфера обмена и показа секрета
 * (F13, «Настройки → Безопасность»).
 *
 * ЗАКОН №1: здесь три числа и ничего больше. Ни PIN, ни мастер-пароля в этом
 * сторе нет и быть не может — у него просто нет для них поля.
 *
 * Стор — ЕДИНСТВЕННЫЙ, кто пишет в `securityPolicy()`. Оттуда действующие
 * значения читают таймеры `useRecordSecrets` и `useClipboard`, которые про
 * Pinia ничего не знают (и не должны — они живут и в контекстах без неё).
 *
 * Своего таймера автоблокировки здесь нет: бездействие считает ЯДРО и присылает
 * `locked` с `reason: 'timeout'`. `autolock_ms` фронт только показывает.
 */
export const useSecurityStore = defineStore('security', () => {
  /**
   * Настройки, известные фронту. Стартуют с умолчаний, а не с `null`: таймеры
   * работают с первой секунды, и им нужно осмысленное значение до ответа ядра.
   * Умолчания равны тем числам, по которым UI работал до появления настроек, —
   * поэтому «ещё не загрузились» неотличимо от обычной работы.
   */
  const settings = ref<SecuritySettings>({ ...DEFAULT_SECURITY_SETTINGS })
  const loading = ref(false)
  const saving = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)
  const loaded = ref(false)

  let unsubscribe: Unsubscribe | null = null

  /**
   * Настройки — содержимое хранилища, за замком их знать незачем. Политика при
   * этом возвращается к умолчаниям: закрытому хранилищу чужие таймауты не нужны,
   * а таймеры должны остаться рабочими.
   */
  function watchCore(): void {
    if (unsubscribe) return
    unsubscribe = useCore().on('locked', () => {
      clear()
    })
  }

  function clear(): void {
    settings.value = { ...DEFAULT_SECURITY_SETTINGS }
    loaded.value = false
    error.value = null
    resetSecurityPolicy()
  }

  /** Забрать настройки у ядра один раз за сеанс. */
  async function ensure(): Promise<SecuritySettings> {
    if (loaded.value) return settings.value
    return load()
  }

  async function load(): Promise<SecuritySettings> {
    watchCore()
    loading.value = true
    error.value = null
    try {
      apply(await useCore().getSecuritySettings())
      loaded.value = true
    } catch (cause) {
      // Не смогли спросить — остаёмся на умолчаниях. Это не отказ экрана:
      // таймеры продолжают работать по тем же числам, что и раньше.
      error.value = isCoreError(cause)
        ? cause.message
        : 'Не удалось получить настройки безопасности.'
    } finally {
      loading.value = false
    }
    return settings.value
  }

  /**
   * Изменить одну или несколько настроек. Ошибку отдаём исключением: показать
   * её должен экран рядом с переключателем, а не подменив собой настройки.
   */
  async function save(patch: SecuritySettingsPatch): Promise<SecuritySettings> {
    watchCore()
    saving.value = true
    error.value = null
    try {
      // В состояние идёт ответ ЯДРА, а не отправленный патч: нормализацию и
      // проверку делает оно, и показывать надо то, что оно приняло.
      apply(await useCore().saveSecuritySettings(patch))
      loaded.value = true
      return settings.value
    } finally {
      saving.value = false
    }
  }

  function apply(next: SecuritySettings): void {
    settings.value = next
    setSecurityPolicy(next)
  }

  /** Снять подписку на события ядра — нужно тестам и горячей перезагрузке. */
  function dispose(): void {
    unsubscribe?.()
    unsubscribe = null
  }

  return { settings, loading, saving, error, loaded, ensure, load, save, clear, dispose }
})
