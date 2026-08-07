import { defineStore } from 'pinia'
import { ref } from 'vue'

import type { GeneratorProfile } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Профиль генератора паролей (F6, §6.1).
 *
 * ЗАКОН №1: здесь лежат ТОЛЬКО правила — длина, наборы символов, число слов.
 * Ни одного сгенерированного пароля в этом сторе нет и быть не может: у него
 * просто нет для них поля. Сами варианты живут в `usePasswordGenerator`, в
 * области видимости компонента. Проверяется тестом.
 *
 * Почему стор, а не локальное состояние: профиль настраивается ОДИН РАЗ (§6.1)
 * и нужен сразу двум экранам — форме записи и настройкам. Держать его копию в
 * каждом означало бы показывать в форме правила, которые пользователь только
 * что поменял в настройках.
 */
export const useGeneratorStore = defineStore('generator', () => {
  /** `null` — у ядра ещё не спрашивали. */
  const profile = ref<GeneratorProfile | null>(null)
  const loading = ref(false)
  const saving = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)
  const loaded = ref(false)

  let unsubscribe: Unsubscribe | null = null

  /** Профиль — содержимое хранилища: за замком он остаться не должен. */
  function watchCore(): void {
    if (unsubscribe) return
    unsubscribe = useCore().on('locked', () => {
      clear()
    })
  }

  function clear(): void {
    profile.value = null
    loaded.value = false
    error.value = null
  }

  /**
   * Забрать профиль у ядра. Повторные вызовы ничего не делают: экраны зовут
   * `ensure()` при монтировании, и перезапрашивать одни и те же правила при
   * каждом открытии формы незачем.
   */
  async function ensure(): Promise<GeneratorProfile | null> {
    if (loaded.value) return profile.value
    return load()
  }

  async function load(): Promise<GeneratorProfile | null> {
    watchCore()
    loading.value = true
    error.value = null
    try {
      profile.value = await useCore().getGeneratorProfile()
      loaded.value = true
    } catch (cause) {
      profile.value = null
      loaded.value = false
      error.value = isCoreError(cause) ? cause.message : 'Не удалось получить профиль генератора.'
    } finally {
      loading.value = false
    }
    return profile.value
  }

  /**
   * Сохранить правила. Ошибку наружу отдаём исключением: показать её должен
   * экран рядом с настройками, а не подменив их сообщением.
   */
  async function save(next: GeneratorProfile): Promise<GeneratorProfile> {
    watchCore()
    saving.value = true
    error.value = null
    try {
      const stored = await useCore().saveGeneratorProfile(next)
      profile.value = stored
      loaded.value = true
      return stored
    } finally {
      saving.value = false
    }
  }

  /** Снять подписку на события ядра — нужно тестам и горячей перезагрузке. */
  function dispose(): void {
    unsubscribe?.()
    unsubscribe = null
  }

  return { profile, loading, saving, error, loaded, ensure, load, save, clear, dispose }
})
