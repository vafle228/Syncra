import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { effectScope } from 'vue'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_MASTER_PASSWORD, type MockCoreClient } from '@/core/mock'

import { DEFAULT_IMPORT_OPTIONS, useVaultExport, useVaultImport } from '../useDataTransfer'

/**
 * Экспорт и импорт глазами экрана (F12).
 *
 * ЗАКОН №1 здесь про два разных следа. Разобранный файл — это чужие пароли,
 * ждущие в ядре согласия; путь к CSV — адрес файла, в котором лежат все пароли
 * открытым текстом. Ни то, ни другое не должно пережить экран или замок.
 */

let core: MockCoreClient

function runExport(kind: 'csv' | 'backup') {
  const scope = effectScope()
  const state = scope.run(() => useVaultExport(kind))!
  return { state, stop: () => scope.stop() }
}

function runImport() {
  const scope = effectScope()
  const state = scope.run(() => useVaultImport())!
  return { state, stop: () => scope.stop() }
}

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

describe('экспорт', () => {
  it('не идёт в ядро с пустым паролем', async () => {
    const { state, stop } = runExport('csv')

    expect(await state.run('')).toBe(false)
    expect(state.error.value).toBe('Введите мастер-пароль.')
    expect(state.file.value).toBeNull()

    stop()
  })

  it('держит след файла, пока хранилище открыто', async () => {
    const { state, stop } = runExport('csv')

    expect(await state.run(MOCK_MASTER_PASSWORD)).toBe(true)
    expect(state.file.value?.path).toContain('syncra-plain-')

    // Замок уносит с экрана и напоминание о файле: экран всё равно закрывается.
    core.control.forceLock('timeout')
    expect(state.file.value).toBeNull()

    stop()
  })

  it('удаляет файл руками ядра', async () => {
    const { state, stop } = runExport('csv')
    await state.run(MOCK_MASTER_PASSWORD)
    const path = state.file.value!.path

    expect(await state.remove()).toBe(true)
    expect(state.file.value).toBeNull()
    // Ядро правда его забыло: второй раз удалять нечего.
    await expect(core.deleteExport(path)).rejects.toMatchObject({ code: 'NOT_FOUND' })

    stop()
  })
})

describe('импорт', () => {
  it('держит предпросмотр без паролей и доводит его до записей', async () => {
    const { state, stop } = runImport()

    expect(await state.pick('chrome')).toBe(true)
    expect(JSON.stringify(state.preview.value)).not.toContain('mock-import-')

    expect(await state.commit()).toBe(true)
    expect(state.preview.value).toBeNull()
    expect(state.result.value?.imported).toBeGreaterThan(0)

    stop()
  })

  it('не считает закрытое окно выбора ошибкой', async () => {
    const { state, stop } = runImport()
    core.control.cancelNextFilePick()

    expect(await state.pick('chrome')).toBe(false)
    expect(state.error.value).toBeNull()
    expect(state.preview.value).toBeNull()

    stop()
  })

  it('уход с экрана заставляет ядро забыть разобранный файл', async () => {
    const { state, stop } = runImport()
    await state.pick('chrome')
    const session = state.preview.value!.session_id

    stop()
    await Promise.resolve()

    await expect(core.commitImport(session, DEFAULT_IMPORT_OPTIONS)).rejects.toMatchObject({
      code: 'NOT_FOUND',
    })
  })

  it('замок стирает разобранный файл с экрана', async () => {
    const { state, stop } = runImport()
    await state.pick('chrome')

    core.control.forceLock('system')

    expect(state.preview.value).toBeNull()
    expect(state.result.value).toBeNull()

    stop()
  })
})
