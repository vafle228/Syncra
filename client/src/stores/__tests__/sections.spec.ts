import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_VAULT_PERSONAL, MOCK_VAULT_WORK } from '@/core/mock'
import type { MockCoreClient } from '@/core/mock'
import { useRecordsStore } from '../useRecordsStore'
import { useSectionsStore } from '../useSectionsStore'

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

describe('загрузка секций', () => {
  it('забирает список у ядра и находит секцию по умолчанию', async () => {
    const sections = useSectionsStore()

    await sections.load()

    expect(sections.loaded).toBe(true)
    expect(sections.vaults.map((vault) => vault.name)).toEqual(['Личное', 'Рабочее'])
    expect(sections.defaultVault?.vault_id).toBe(MOCK_VAULT_PERSONAL)
    expect(sections.hasLocal).toBe(true)
    expect(sections.byId(MOCK_VAULT_WORK)?.name).toBe('Рабочее')
    expect(sections.byId(null)).toBeNull()
  })

  it('ensure() не ходит в ядро повторно', async () => {
    const sections = useSectionsStore()
    await sections.ensure()

    // Секция создана мимо стора: если бы ensure() перезапрашивал, она бы
    // появилась в списке.
    await core.createVault('Учёба', 'mint')
    await sections.ensure()

    expect(sections.vaults).toHaveLength(2)
    await sections.load()
    expect(sections.vaults).toHaveLength(3)
  })

  it('показывает сообщение ядра и оставляет список пустым при ошибке', async () => {
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    const sections = useSectionsStore()

    await sections.load()

    expect(sections.error).toBe('Ядро недоступно.')
    expect(sections.vaults).toHaveLength(0)
    expect(sections.loaded).toBe(false)
  })
})

describe('изменения секций', () => {
  it('создаёт секцию и добавляет её в конец списка', async () => {
    const sections = useSectionsStore()
    await sections.load()

    const created = await sections.create('Учёба', 'mint')

    expect(created.sync).toBe(true)
    expect(sections.vaults.map((vault) => vault.name)).toEqual(['Личное', 'Рабочее', 'Учёба'])
  })

  it('переименовывает секцию на месте', async () => {
    const sections = useSectionsStore()
    await sections.load()

    await sections.update(MOCK_VAULT_WORK, { name: 'Работа', color: 'coral' })

    expect(sections.byId(MOCK_VAULT_WORK)?.name).toBe('Работа')
    expect(sections.byId(MOCK_VAULT_WORK)?.color).toBe('coral')
    expect(sections.vaults).toHaveLength(2)
  })

  it('сохраняет флаг синхронизации (§4.2)', async () => {
    const sections = useSectionsStore()
    await sections.load()

    await sections.setSync(MOCK_VAULT_WORK, true)
    expect(sections.byId(MOCK_VAULT_WORK)?.sync).toBe(true)
    expect(sections.hasLocal).toBe(false)

    // Перечитали у ядра — флаг тот же, а не только в памяти UI.
    await sections.load()
    expect(sections.byId(MOCK_VAULT_WORK)?.sync).toBe(true)
  })

  it('переносит пометку «по умолчанию» ровно на одну секцию', async () => {
    const sections = useSectionsStore()
    await sections.load()

    await sections.setDefault(MOCK_VAULT_WORK)

    expect(sections.defaultVault?.vault_id).toBe(MOCK_VAULT_WORK)
    expect(sections.vaults.filter((vault) => vault.is_default)).toHaveLength(1)
  })

  it('удаляет секцию, не трогая записи', async () => {
    const sections = useSectionsStore()
    const list = useRecordsStore()
    await sections.load()
    await list.load()
    const before = list.totalAll

    await sections.remove(MOCK_VAULT_WORK)
    await list.load()

    expect(sections.vaults.map((vault) => vault.vault_id)).toEqual([MOCK_VAULT_PERSONAL])
    expect(list.totalAll).toBe(before)
    expect(list.records.every((record) => record.vault_id === MOCK_VAULT_PERSONAL)).toBe(true)
  })

  it('пробрасывает отказ ядра наружу, не гася список', async () => {
    const sections = useSectionsStore()
    await sections.load()

    await expect(sections.remove(MOCK_VAULT_PERSONAL)).rejects.toMatchObject({
      code: 'VALIDATION',
    })
    expect(sections.error).toBeNull()
    expect(sections.vaults).toHaveLength(2)
  })
})

describe('замок', () => {
  it('очищает список секций по событию блокировки', async () => {
    const sections = useSectionsStore()
    await sections.load()
    expect(sections.vaults).toHaveLength(2)

    core.control.forceLock('timeout')

    expect(sections.vaults).toHaveLength(0)
    expect(sections.loaded).toBe(false)
  })

  it('после dispose события больше не двигают состояние', async () => {
    const sections = useSectionsStore()
    await sections.load()

    sections.dispose()
    core.control.forceLock('system')

    expect(sections.vaults).toHaveLength(2)
  })
})
