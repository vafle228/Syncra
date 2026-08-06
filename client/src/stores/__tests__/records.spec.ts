import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'
import { useRecordsStore } from '../useRecordsStore'

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

describe('загрузка списка', () => {
  it('забирает метаданные у ядра и не показывает надгробия', async () => {
    const list = useRecordsStore()
    expect(list.loaded).toBe(false)

    await list.load()

    expect(list.loading).toBe(false)
    expect(list.loaded).toBe(true)
    // В сиде пять записей, одна из них — tombstone (§5.4).
    expect(list.total).toBe(4)
    expect(list.records.every((record) => record.deleted_at === null)).toBe(true)
    expect(list.error).toBeNull()
  })

  it('показывает сообщение ядра и оставляет список пустым при ошибке', async () => {
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    const list = useRecordsStore()

    await list.load()

    expect(list.error).toBe('Ядро недоступно.')
    expect(list.total).toBe(0)
    expect(list.loaded).toBe(false)
  })

  it('на заблокированном хранилище отдаёт сообщение ядра, а не падает', async () => {
    await core.lock()
    const list = useRecordsStore()

    await list.load()

    expect(list.error).toBe('Хранилище заблокировано.')
    expect(list.total).toBe(0)
  })
})

describe('поиск и группировка', () => {
  it('фильтрует по запросу и считает найденное', async () => {
    const list = useRecordsStore()
    await list.load()

    list.setQuery('google')

    expect(list.isSearching).toBe(true)
    expect(list.visible).toBe(2)
    expect(list.total).toBe(4)

    list.setQuery('google рабочий')
    expect(list.visible).toBe(1)

    list.clearQuery()
    expect(list.isSearching).toBe(false)
    expect(list.visible).toBe(4)
  })

  it('собирает два аккаунта Google в одну группу (§4.4)', async () => {
    const list = useRecordsStore()
    await list.load()

    const google = list.groups.find((group) => group.title === 'Google')

    expect(google?.records).toHaveLength(2)
    expect(google?.records.map((record) => record.login).sort()).toEqual([
      'personal.demo@gmail.com',
      'work.demo@syncra.example',
    ])
    // Четыре записи — три сервиса: Google, GitHub, Steam.
    expect(list.serviceCount).toBe(3)
  })

  it('ищет по адресу, которого нет в имени сервиса', async () => {
    const list = useRecordsStore()
    await list.load()

    list.setQuery('steampowered')

    expect(list.visible).toBe(1)
    expect(list.matched[0]?.service_name).toBe('Steam')
  })
})

describe('ЗАКОН №1', () => {
  it('в состоянии стора нет ни одного секрета', async () => {
    const list = useRecordsStore()
    await list.load()

    const snapshot = JSON.stringify(list.$state)

    // Все сид-секреты помечены `mock-…` / MOCKTOTP.
    expect(snapshot).not.toMatch(/mock-[a-z]+-pw/)
    expect(snapshot).not.toContain('MOCKTOTPSECRET')
    expect(snapshot).not.toContain('Recovery codes')
  })

  it('очищает список по событию блокировки', async () => {
    const list = useRecordsStore()
    await list.load()
    list.setQuery('google')
    expect(list.total).toBe(4)

    // Заблокировать может не только пользователь: таймаут, сон системы.
    core.control.forceLock('timeout')

    expect(list.total).toBe(0)
    expect(list.query).toBe('')
    expect(list.loaded).toBe(false)
  })

  it('после dispose события больше не двигают состояние', async () => {
    const list = useRecordsStore()
    await list.load()

    list.dispose()
    core.control.forceLock('system')

    expect(list.total).toBe(4)
  })
})
