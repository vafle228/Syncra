import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import type { RecordId, SyncStatus } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Что происходит с синхронизацией (F10, §5.1 · §5.3).
 *
 * ЗАКОН №1: секретов здесь нет и взяться неоткуда — `SyncStatus` их не содержит
 * по контракту. `pending_records` — это идентификаторы, а не содержимое.
 *
 * Стор живёт СОБЫТИЯМИ: команда `get_sync_status` спрашивается один раз, чтобы
 * знать состояние до первого события (а его может не быть часами), дальше ядро
 * присылает полный статус само. Опрашивать ядро по таймеру не нужно — и не
 * стоит: это была бы вторая правда о том же самом.
 *
 * По событию `locked` статус очищается наравне со списком записей: «две записи
 * ждут отправки» и «рядом iPhone 14» — сведения о хранилище и о том, чем
 * пользуется владелец.
 */
export const useSyncStore = defineStore('sync', () => {
  const status = ref<SyncStatus | null>(null)
  const loading = ref(false)
  /** Был ли хоть один успешный ответ ядра. */
  const loaded = ref(false)
  /** Сообщение ядра о неудавшемся ЗАПРОСЕ. Обрыв обмена — это `status.message`. */
  const error = ref<string | null>(null)

  /** Сколько изменений ещё не уехало. */
  const pendingCount = computed(() => status.value?.pending_records.length ?? 0)
  const peersOnline = computed(() => status.value?.peers_online ?? 0)

  /** Эта запись ждёт отправки — строка списка помечается «ждёт синхронизации». */
  function isPending(recordId: RecordId): boolean {
    return status.value?.pending_records.includes(recordId) ?? false
  }

  let unsubscribes: Unsubscribe[] = []

  function watchCore(): void {
    if (unsubscribes.length > 0) return
    const core = useCore()
    unsubscribes = [
      // Событие несёт полный статус: собирать его из кусочков не нужно.
      core.on('sync_status', (next) => {
        status.value = next
        error.value = null
      }),
      core.on('locked', () => {
        clear()
      }),
    ]
  }

  function clear(): void {
    status.value = null
    loaded.value = false
    error.value = null
  }

  /**
   * Спросить ядро, если ещё не спрашивали. Дальше состояние приезжает
   * событиями, поэтому второй запрос не нужен — а экранов, которым нужен
   * статус, несколько.
   */
  async function ensure(): Promise<void> {
    if (loaded.value) return
    await load()
  }

  async function load(): Promise<void> {
    watchCore()
    loading.value = true
    error.value = null
    try {
      status.value = await useCore().getSyncStatus()
      loaded.value = true
    } catch (cause) {
      status.value = null
      loaded.value = false
      error.value = isCoreError(cause)
        ? cause.message
        : 'Не удалось узнать состояние синхронизации.'
    } finally {
      loading.value = false
    }
  }

  /**
   * «Попробовать сейчас» — только для состояния «не удалось». У «рядом никого»
   * повторять нечего: там не ошибка, а отсутствие второго устройства, и кнопку
   * там макет запрещает намеренно.
   */
  async function retry(): Promise<void> {
    watchCore()
    loading.value = true
    error.value = null
    try {
      status.value = await useCore().syncNow()
    } catch (cause) {
      error.value = isCoreError(cause) ? cause.message : 'Не удалось начать обмен.'
    } finally {
      loading.value = false
    }
  }

  /** Снять подписки на события ядра — нужно тестам и горячей перезагрузке. */
  function dispose(): void {
    for (const unsubscribe of unsubscribes) unsubscribe()
    unsubscribes = []
  }

  return {
    status,
    loading,
    loaded,
    error,
    pendingCount,
    peersOnline,
    isPending,
    ensure,
    load,
    retry,
    clear,
    dispose,
  }
})
