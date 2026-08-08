import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_DEVICE_PHONE, type MockCoreClient } from '@/core/mock'

import { useSyncStore } from '../useSyncStore'

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  core.control.dispose()
  setCoreClient(null)
})

describe('первая загрузка', () => {
  it('спрашивает ядро один раз, дальше живёт событиями', async () => {
    const sync = useSyncStore()

    await sync.load()
    expect(sync.status?.phase).toBe('idle')
    expect(sync.peersOnline).toBe(0)

    // Команду больше никто не звал — состояние приехало само.
    core.control.peerFound(MOCK_DEVICE_PHONE)
    core.control.startSync(MOCK_DEVICE_PHONE)

    expect(sync.status?.phase).toBe('syncing')
    expect(sync.status?.peer_name).toBe('iPhone 14')
    expect(sync.peersOnline).toBe(1)
  })

  it('ensure() не ходит в ядро повторно: дальше всё приезжает событиями', async () => {
    const sync = useSyncStore()
    await sync.ensure()

    core.control.failNext('INTERNAL', 'Второго запроса быть не должно.')
    await sync.ensure()

    expect(sync.error).toBeNull()
    expect(sync.status?.phase).toBe('idle')
  })

  it('показывает сообщение ядра и не выдумывает состояние', async () => {
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    const sync = useSyncStore()

    await sync.load()

    expect(sync.error).toBe('Ядро недоступно.')
    expect(sync.status).toBeNull()
  })
})

describe('ожидающие изменения', () => {
  it('знает поимённо, какая запись ещё не уехала', async () => {
    const sync = useSyncStore()
    await sync.load()

    const created = await core.createRecord({
      service_name: 'Fastmail',
      urls: ['app.fastmail.com'],
      login: 'demo@fastmail.example',
      password: 'mock-fastmail-pw',
    })

    expect(sync.pendingCount).toBe(1)
    expect(sync.isPending(created.record_id)).toBe(true)
    expect(sync.isPending('нет-такой-записи')).toBe(false)
  })
})

describe('повторная попытка', () => {
  it('после обрыва просит ядро начать заново', async () => {
    const sync = useSyncStore()
    await sync.load()
    core.control.finishSync('Соединение оборвалось.')
    expect(sync.status?.phase).toBe('error')

    await sync.retry()

    expect(sync.status?.phase).toBe('searching')
    expect(sync.status?.message).toBeNull()
  })

  it('отказ ядра не стирает того, что уже известно', async () => {
    const sync = useSyncStore()
    await sync.load()
    core.control.finishSync('Соединение оборвалось.')

    core.control.failNext('INTERNAL', 'Ядро занято.')
    await sync.retry()

    expect(sync.error).toBe('Ядро занято.')
    expect(sync.status?.phase).toBe('error')
  })
})

describe('замок', () => {
  it('очищает состояние синхронизации', async () => {
    const sync = useSyncStore()
    await sync.load()
    core.control.peerFound(MOCK_DEVICE_PHONE)
    expect(sync.status).not.toBeNull()

    core.control.forceLock('timeout')

    expect(sync.status).toBeNull()
    expect(sync.pendingCount).toBe(0)
  })

  it('после dispose события больше не двигают состояние', async () => {
    const sync = useSyncStore()
    await sync.load()

    sync.dispose()
    core.control.peerFound(MOCK_DEVICE_PHONE)

    expect(sync.peersOnline).toBe(0)
  })
})
