import { defineStore } from 'pinia'
import { ref, watch } from 'vue'

import type { RecordId } from '@/core/contract'
import { useCore, type Unsubscribe } from '@/core/ipc'
import { useRecordsStore } from './useRecordsStore'

/**
 * Состояние оболочки хранилища: что показывает правая панель и висит ли баннер
 * после импорта (F13).
 *
 * Почему стор, а не локальный `ref`. До F13 режим правой панели жил внутри
 * `HomeView`, который был сразу и списком, и панелью. В окне прототипа это
 * разные компоненты: кнопка «+» стоит в `RecordList` (средняя панель, часть
 * оболочки), а форма открывается в `VaultView` (дочерний роут). Они СИБЛИНГИ —
 * прокинуть между ними пропсом нечего.
 *
 * ЗАКОН №1: здесь нет ни одного секрета и нет даже черновика записи. Только
 * «какой сейчас режим» и «сколько записей приехало импортом». Черновик формы
 * живёт внутри `RecordForm` и умирает вместе с ней — на этом держится обещание,
 * что уход с экрана гасит незаписанное.
 */

/** Что показывает правая панель на `/`. */
export type EditorMode = 'none' | 'create' | 'edit'

export const useVaultUiStore = defineStore('vaultUi', () => {
  const records = useRecordsStore()

  const editor = ref<EditorMode>('none')
  /**
   * Баннер «приехало N записей» над списком. Держим счётчик, а не флаг: текст
   * баннера называет число, и хранить его где-то ещё значило бы иметь две правды.
   */
  const importBanner = ref<{ count: number } | null>(null)

  let unsubscribe: Unsubscribe | null = null

  /** Замок закрывает и оболочку: открытая форма за ним не переживает. */
  function watchCore(): void {
    if (unsubscribe) return
    unsubscribe = useCore().on('locked', () => {
      clear()
    })
  }

  function clear(): void {
    editor.value = 'none'
    importBanner.value = null
  }

  /**
   * Открыть запись. Выбор ставит стор записей — здесь только гасится редактор:
   * иначе клик по соседней строке оставил бы на экране форму от прошлой.
   */
  function openRecord(id: RecordId): void {
    watchCore()
    records.select(id)
    editor.value = 'none'
  }

  /** Завести новую запись. Выбор снимается: форма создания ничью карточку не правит. */
  function startCreate(): void {
    watchCore()
    records.select(null)
    editor.value = 'create'
  }

  function startEdit(): void {
    watchCore()
    editor.value = 'edit'
  }

  function closeEditor(): void {
    editor.value = 'none'
  }

  function showImportBanner(count: number): void {
    watchCore()
    importBanner.value = { count }
  }

  function hideImportBanner(): void {
    importBanner.value = null
  }

  /**
   * Человек ушёл искать другое — форма создания не должна висеть поверх выдачи.
   *
   * Наблюдатель стоит здесь, а не в компоненте поиска, ровно потому, что поиск и
   * форма теперь в разных панелях: связывать их через DOM значило бы полагаться
   * на то, что обе смонтированы.
   */
  watch(
    () => records.query,
    () => {
      if (editor.value === 'create') editor.value = 'none'
    },
  )

  /** Снять подписку на события ядра — нужно тестам и горячей перезагрузке. */
  function dispose(): void {
    unsubscribe?.()
    unsubscribe = null
  }

  return {
    editor,
    importBanner,
    openRecord,
    startCreate,
    startEdit,
    closeEditor,
    showImportBanner,
    hideImportBanner,
    clear,
    dispose,
  }
})
