import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { useRecordList } from '@/composables/useRecordList'
import type { RecordDraft, RecordId, RecordMeta, RecordPatch } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Список записей главного экрана и CRUD над ними (F4, F5).
 *
 * ЗАКОН №1: здесь лежат ТОЛЬКО метаданные — `RecordMeta` секретных полей не
 * содержит по контракту, и класть их сюда нечем. Черновик записи (`RecordDraft`
 * с паролем и заметками) проходит через `create` / `update` ТРАНЗИТОМ: уходит
 * в ядро аргументом и не оседает в состоянии. Reveal идёт вообще мимо стора —
 * `useRecordSecrets` запрашивает секрет разово и держит его в области видимости
 * компонента. Обе гарантии проверяются тестами.
 *
 * Метаданные — тоже содержимое хранилища, поэтому пережить блокировку они не
 * должны: по событию `locked` список очищается.
 */

/** Порядок ядра: по имени сервиса, затем по логину. Держим его же локально. */
function sortRecords(records: RecordMeta[]): RecordMeta[] {
  return [...records].sort(
    (a, b) => a.service_name.localeCompare(b.service_name) || a.login.localeCompare(b.login),
  )
}

export const useRecordsStore = defineStore('records', () => {
  const records = ref<RecordMeta[]>([])
  const query = ref('')
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)
  const loading = ref(false)
  /** Был ли хоть один успешный ответ ядра — отличает «пусто» от «ещё не спрашивали». */
  const loaded = ref(false)
  /** Какая запись открыта в правой панели. */
  const selectedId = ref<RecordId | null>(null)

  const { matched, groups, isSearching } = useRecordList(records, query)

  const total = computed(() => records.value.length)
  const visible = computed(() => matched.value.length)
  /** Сколько сервисов в выдаче — группа §4.4 считается за один. */
  const serviceCount = computed(() => groups.value.length)

  const selected = computed(
    () => records.value.find((record) => record.record_id === selectedId.value) ?? null,
  )

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
    selectedId.value = null
  }

  /** Забрать метаданные записей у ядра. */
  async function load(): Promise<void> {
    watchCore()
    loading.value = true
    error.value = null
    try {
      records.value = await useCore().listRecords()
      loaded.value = true
      // Запись могли удалить на другом устройстве — выбор не должен «висеть».
      if (selected.value === null) selectedId.value = null
    } catch (cause) {
      records.value = []
      loaded.value = false
      selectedId.value = null
      error.value = isCoreError(cause) ? cause.message : 'Не удалось связаться с ядром.'
    } finally {
      loading.value = false
    }
  }

  function select(id: RecordId | null): void {
    selectedId.value = id
  }

  // -------------------------------------------------------------------------
  // CRUD (F5)
  //
  // Ошибки НЕ пишутся в `error`: он гасит список и показывает вместо него
  // сообщение. Ошибку сохранения должна показать форма, рядом с полями, —
  // поэтому наружу летит исключение.
  // -------------------------------------------------------------------------

  /** Создать запись. `draft` содержит секреты и здесь не сохраняется. */
  async function create(draft: RecordDraft): Promise<RecordMeta> {
    const created = await useCore().createRecord(draft)
    records.value = sortRecords([...records.value, created])
    selectedId.value = created.record_id
    return created
  }

  /** Изменить запись. `patch` содержит секреты и здесь не сохраняется. */
  async function update(id: RecordId, patch: RecordPatch): Promise<RecordMeta> {
    const updated = await useCore().updateRecord(id, patch)
    records.value = sortRecords(
      records.value.map((record) => (record.record_id === id ? updated : record)),
    )
    return updated
  }

  /**
   * Удалить запись. Ядро ставит tombstone (§5.4) и возвращает его метаданные;
   * в живом списке надгробию делать нечего — убираем строку.
   */
  async function remove(id: RecordId): Promise<void> {
    await useCore().deleteRecord(id)
    records.value = records.value.filter((record) => record.record_id !== id)
    if (selectedId.value === id) selectedId.value = null
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
    selectedId,
    selected,
    groups,
    matched,
    isSearching,
    total,
    visible,
    serviceCount,
    load,
    select,
    create,
    update,
    remove,
    setQuery,
    clearQuery,
    clear,
    dispose,
  }
})
