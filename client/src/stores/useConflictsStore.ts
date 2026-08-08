import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import type { ConflictSide, RecordConflict, RecordId, RecordMeta } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'
import { useRecordsStore } from '@/stores/useRecordsStore'

/**
 * Записи, разошедшиеся на двух устройствах (F11, §5.5).
 *
 * ЗАКОН №1: `RecordConflict` не содержит секретных значений — только имена
 * полей, по которым версии расходятся. Сами значения открываются разово и
 * отдельно (`useConflictSecrets`) и в стор не попадают.
 *
 * Конфликт НИЧЕГО не блокирует: он ждёт в списке столько, сколько нужно
 * (правило из макета — «индикатор не показывает модальных окон»). Поэтому
 * здесь нет ни таймеров, ни автоподстановки «более свежей» версии: выбирает
 * человек, и только он.
 *
 * По событию `locked` список очищается наравне с записями: какие записи
 * разошлись — это тоже содержимое хранилища.
 */
export const useConflictsStore = defineStore('conflicts', () => {
  const conflicts = ref<RecordConflict[]>([])
  const loading = ref(false)
  /** Был ли хоть один успешный ответ ядра. */
  const loaded = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)

  const count = computed(() => conflicts.value.length)
  const hasConflicts = computed(() => conflicts.value.length > 0)

  /** Конфликт этой записи — карточка рисует по нему предупреждение. */
  function byRecord(recordId: RecordId | null): RecordConflict | null {
    if (recordId === null) return null
    return conflicts.value.find((conflict) => conflict.record_id === recordId) ?? null
  }

  let unsubscribes: Unsubscribe[] = []

  function watchCore(): void {
    if (unsubscribes.length > 0) return
    const core = useCore()
    unsubscribes = [
      core.on('conflict_raised', (conflict) => {
        // Тот же record_id мог уже конфликтовать: ядро прислало более свежую
        // картину, а не второй конфликт по той же записи.
        conflicts.value = [
          ...conflicts.value.filter((item) => item.record_id !== conflict.record_id),
          conflict,
        ]
      }),
      core.on('locked', () => {
        clear()
      }),
    ]
  }

  function clear(): void {
    conflicts.value = []
    loaded.value = false
    error.value = null
  }

  async function ensure(): Promise<RecordConflict[]> {
    if (loaded.value) return conflicts.value
    return load()
  }

  async function load(): Promise<RecordConflict[]> {
    watchCore()
    loading.value = true
    error.value = null
    try {
      conflicts.value = await useCore().listConflicts()
      loaded.value = true
    } catch (cause) {
      conflicts.value = []
      loaded.value = false
      error.value = isCoreError(cause) ? cause.message : 'Не удалось получить список конфликтов.'
    } finally {
      loading.value = false
    }
    return conflicts.value
  }

  /**
   * Оставить одну версию целиком (§5.5).
   *
   * Ошибка летит наружу исключением и НЕ пишется в `error`: тот гасит экран, а
   * отказ по конкретной записи должен быть виден в её же диалоге.
   *
   * Победившая версия сразу кладётся в список записей: у неё могли смениться
   * логин и адреса, и оставлять на экране проигравшие данные было бы враньём о
   * том, что человек только что выбрал.
   */
  async function resolve(recordId: RecordId, side: ConflictSide): Promise<RecordMeta> {
    const resolved = await useCore().resolveConflict(recordId, side)
    conflicts.value = conflicts.value.filter((conflict) => conflict.record_id !== recordId)
    useRecordsStore().replace(resolved)
    return resolved
  }

  /** Снять подписки на события ядра — нужно тестам и горячей перезагрузке. */
  function dispose(): void {
    for (const unsubscribe of unsubscribes) unsubscribe()
    unsubscribes = []
  }

  return {
    conflicts,
    loading,
    loaded,
    error,
    count,
    hasConflicts,
    byRecord,
    ensure,
    load,
    resolve,
    clear,
    dispose,
  }
})
