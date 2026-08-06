import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { useRecordList } from '@/composables/useRecordList'
import type { RecordMeta } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Список записей главного экрана (F4).
 *
 * ЗАКОН №1: здесь лежат ТОЛЬКО метаданные — `RecordMeta` секретных полей не
 * содержит по контракту, и класть их сюда нечем. Reveal (F5) работает мимо
 * стора: разовый `getSecret` и сразу отпустить.
 *
 * Метаданные — тоже содержимое хранилища, поэтому пережить блокировку они не
 * должны: по событию `locked` список очищается.
 */
export const useRecordsStore = defineStore('records', () => {
  const records = ref<RecordMeta[]>([])
  const query = ref('')
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)
  const loading = ref(false)
  /** Был ли хоть один успешный ответ ядра — отличает «пусто» от «ещё не спрашивали». */
  const loaded = ref(false)

  const { matched, groups, isSearching } = useRecordList(records, query)

  const total = computed(() => records.value.length)
  const visible = computed(() => matched.value.length)
  /** Сколько сервисов в выдаче — группа §4.4 считается за один. */
  const serviceCount = computed(() => groups.value.length)

  let unsubscribe: Unsubscribe | null = null

  /** Блокировка закрывает не только секреты: список метаданных тоже уходит. */
  function watchCore(): void {
    if (unsubscribe) return
    unsubscribe = useCore().on('locked', () => {
      clear()
    })
  }

  function clear(): void {
    records.value = []
    query.value = ''
    error.value = null
    loaded.value = false
  }

  /** Забрать метаданные записей у ядра. */
  async function load(): Promise<void> {
    watchCore()
    loading.value = true
    error.value = null
    try {
      records.value = await useCore().listRecords()
      loaded.value = true
    } catch (cause) {
      records.value = []
      loaded.value = false
      error.value = isCoreError(cause) ? cause.message : 'Не удалось связаться с ядром.'
    } finally {
      loading.value = false
    }
  }

  function setQuery(next: string): void {
    query.value = next
  }

  function clearQuery(): void {
    query.value = ''
  }

  /** Снять подписку на события ядра — нужно тестам и горячей перезагрузке. */
  function dispose(): void {
    unsubscribe?.()
    unsubscribe = null
  }

  return {
    records,
    query,
    error,
    loading,
    loaded,
    groups,
    matched,
    isSearching,
    total,
    visible,
    serviceCount,
    load,
    setQuery,
    clearQuery,
    clear,
    dispose,
  }
})
