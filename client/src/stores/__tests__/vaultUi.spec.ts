import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_RECORD_GITHUB, type MockCoreClient } from '@/core/mock'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useVaultUiStore } from '@/stores/useVaultUiStore'

/**
 * Состояние оболочки (F13): режим правой панели и баннер импорта.
 *
 * Проверяется без монтирования — в этом и смысл того, что режим переехал из
 * локального `ref` в стор: список и правая панель теперь разные компоненты, и
 * связь между ними должна быть проверяемой без обоих сразу.
 */

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

describe('useVaultUiStore · режим правой панели', () => {
  it('начинает с пустой панели: ничья карточка не открыта', () => {
    const ui = useVaultUiStore()

    expect(ui.editor).toBe('none')
    expect(useRecordsStore().selectedId).toBeNull()
  })

  it('«новая запись» снимает выбор — форма создания ничью карточку не правит', () => {
    const ui = useVaultUiStore()
    const list = useRecordsStore()
    list.select(MOCK_RECORD_GITHUB)

    ui.startCreate()

    expect(ui.editor).toBe('create')
    expect(list.selectedId).toBeNull()
  })

  it('открытие записи гасит редактор', () => {
    const ui = useVaultUiStore()
    ui.startCreate()

    ui.openRecord(MOCK_RECORD_GITHUB)

    expect(ui.editor).toBe('none')
    expect(useRecordsStore().selectedId).toBe(MOCK_RECORD_GITHUB)
  })

  it('«изменить» и «отмена» переключают режим, не трогая выбор', () => {
    const ui = useVaultUiStore()
    ui.openRecord(MOCK_RECORD_GITHUB)

    ui.startEdit()
    expect(ui.editor).toBe('edit')

    ui.closeEditor()
    expect(ui.editor).toBe('none')
    expect(useRecordsStore().selectedId).toBe(MOCK_RECORD_GITHUB)
  })
})

describe('useVaultUiStore · поиск гасит форму создания', () => {
  it('смена запроса закрывает незаписанный черновик', async () => {
    // Человек ушёл искать другое — форма создания не должна висеть поверх выдачи.
    const ui = useVaultUiStore()
    ui.startCreate()

    useRecordsStore().setQuery('github')
    await Promise.resolve()

    expect(ui.editor).toBe('none')
  })

  it('но не закрывает форму редактирования: там правят конкретную запись', async () => {
    const ui = useVaultUiStore()
    ui.openRecord(MOCK_RECORD_GITHUB)
    ui.startEdit()

    useRecordsStore().setQuery('git')
    await Promise.resolve()

    expect(ui.editor).toBe('edit')
  })
})

describe('useVaultUiStore · баннер импорта', () => {
  it('запоминает, сколько записей приехало, и снимается по «Понятно»', () => {
    const ui = useVaultUiStore()

    ui.showImportBanner(247)
    expect(ui.importBanner).toEqual({ count: 247 })

    ui.hideImportBanner()
    expect(ui.importBanner).toBeNull()
  })
})

describe('useVaultUiStore · замок', () => {
  it('блокировка закрывает форму и убирает баннер', async () => {
    const ui = useVaultUiStore()
    ui.startCreate()
    ui.showImportBanner(3)

    core.control.forceLock('manual')
    await Promise.resolve()

    expect(ui.editor).toBe('none')
    expect(ui.importBanner).toBeNull()

    ui.dispose()
  })
})
