import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { PeerFoundEvent, SyncStatus } from '../contract'
import { setCoreClient } from '../ipc'
import {
  createMockCoreClient,
  MOCK_DEVICE_LAPTOP,
  MOCK_DEVICE_PHONE,
  MOCK_VAULT_WORK,
  type MockCoreClient,
} from '../mock'

/**
 * Статус синхронизации в фейк-ядре (F10).
 *
 * Проверяется не «сеть работает» (её здесь нет), а то, что UI получает
 * согласованную картину: фазы, счётчики и события не расходятся между собой.
 */

let core: MockCoreClient

beforeEach(() => {
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  core.control.dispose()
  setCoreClient(null)
})

describe('статус синхронизации', () => {
  it('на закрытом хранилище не рассказывает, кто рядом', async () => {
    const locked = createMockCoreClient({ latencyMs: 0 })

    await expect(locked.getSyncStatus()).rejects.toMatchObject({ code: 'LOCKED' })
    await expect(locked.syncNow()).rejects.toMatchObject({ code: 'LOCKED' })
  })

  it('начинает с тишины: ни пиров, ни обмена, ни отправленного', async () => {
    const status = await core.getSyncStatus()

    expect(status).toMatchObject({
      phase: 'idle',
      peers_online: 0,
      peer_name: null,
      pending_records: [],
      last_sync_at: null,
      message: null,
    })
  })

  it('не отдаёт наружу свой объект состояния', async () => {
    const status = await core.getSyncStatus()
    status.pending_records.push('подделка')

    expect((await core.getSyncStatus()).pending_records).toEqual([])
  })
})

describe('изменения, ждущие отправки', () => {
  it('новая запись ждёт синхронизации, пока обмен не состоялся', async () => {
    const created = await core.createRecord({
      service_name: 'Fastmail',
      urls: ['app.fastmail.com'],
      login: 'demo@fastmail.example',
      password: 'mock-fastmail-pw',
    })

    expect((await core.getSyncStatus()).pending_records).toEqual([created.record_id])
  })

  it('запись локальной секции не ждёт ничего: ей некуда ехать (§4.2)', async () => {
    await core.createRecord({
      vault_id: MOCK_VAULT_WORK,
      service_name: 'Внутренний вики',
      urls: ['wiki.local'],
      login: 'demo',
      password: 'mock-wiki-pw',
    })

    expect((await core.getSyncStatus()).pending_records).toEqual([])
  })

  it('удаление тоже должно доехать: надгробие ждёт наравне с правкой (§5.4)', async () => {
    const [record] = await core.listRecords()
    const deleted = await core.deleteRecord(record!.record_id)

    expect((await core.getSyncStatus()).pending_records).toContain(deleted.record_id)
  })

  it('удачный обмен увозит всё, что ждало, и запоминает время', async () => {
    await core.createRecord({
      service_name: 'Fastmail',
      urls: ['app.fastmail.com'],
      login: 'demo@fastmail.example',
      password: 'mock-fastmail-pw',
    })

    core.control.peerFound(MOCK_DEVICE_PHONE)
    core.control.startSync(MOCK_DEVICE_PHONE)
    core.control.finishSync()

    const status = await core.getSyncStatus()
    expect(status.phase).toBe('idle')
    expect(status.pending_records).toEqual([])
    expect(status.last_sync_at).not.toBeNull()
  })

  it('обрыв не трогает очередь: изменения поедут со следующей попыткой', async () => {
    await core.createRecord({
      service_name: 'Fastmail',
      urls: ['app.fastmail.com'],
      login: 'demo@fastmail.example',
      password: 'mock-fastmail-pw',
    })

    core.control.peerFound(MOCK_DEVICE_PHONE)
    core.control.startSync(MOCK_DEVICE_PHONE)
    core.control.finishSync('Соединение оборвалось.')

    const status = await core.getSyncStatus()
    expect(status.phase).toBe('error')
    expect(status.message).toBe('Соединение оборвалось.')
    expect(status.pending_records).toHaveLength(1)
    expect(status.last_sync_at).toBeNull()
  })
})

describe('события', () => {
  it('о найденном устройстве рассказывает событием и двигает «был рядом»', async () => {
    const seen: PeerFoundEvent[] = []
    core.on('peer_found', (event) => seen.push(event))

    core.control.peerFound(MOCK_DEVICE_PHONE)

    expect(seen).toHaveLength(1)
    expect(seen[0]).toMatchObject({ device_id: MOCK_DEVICE_PHONE, name: 'iPhone 14' })

    const devices = await core.listDevices()
    const phone = devices.find((device) => device.device_id === MOCK_DEVICE_PHONE)
    expect(phone?.last_seen_at).toBe(seen[0]?.found_at)
    expect((await core.getSyncStatus()).peers_online).toBe(1)
  })

  it('шлёт полный статус, а не то, что изменилось', async () => {
    const updates: SyncStatus[] = []
    core.on('sync_status', (status) => updates.push(status))

    core.control.peerFound(MOCK_DEVICE_PHONE)
    core.control.startSync(MOCK_DEVICE_PHONE)

    expect(updates.length).toBeGreaterThanOrEqual(2)
    const last = updates[updates.length - 1]
    expect(last).toMatchObject({ phase: 'syncing', peer_name: 'iPhone 14', peers_online: 1 })
    expect(last).toHaveProperty('last_sync_at')
  })

  it('отозванное устройство пиром не считается (§2.3)', async () => {
    await core.revokeDevice(MOCK_DEVICE_LAPTOP)
    const seen: PeerFoundEvent[] = []
    core.on('peer_found', (event) => seen.push(event))

    core.control.peerFound(MOCK_DEVICE_LAPTOP)

    expect(seen).toHaveLength(0)
    expect((await core.getSyncStatus()).peers_online).toBe(0)
  })

  it('отзыв немедленно убирает устройство из тех, кто рядом', async () => {
    core.control.peerFound(MOCK_DEVICE_LAPTOP)
    expect((await core.getSyncStatus()).peers_online).toBe(1)

    await core.revokeDevice(MOCK_DEVICE_LAPTOP)

    expect((await core.getSyncStatus()).peers_online).toBe(0)
  })

  it('сопряжение с той стороны приходит событием: команду здесь никто не вызывал', async () => {
    const paired: string[] = []
    core.on('device_paired', (result) => paired.push(result.device.name))

    const result = core.control.pairedByPeer('Pixel 8')

    expect(paired).toEqual(['Pixel 8'])
    expect(result.records_transferred).toBeGreaterThan(0)
    expect((await core.listDevices()).some((device) => device.name === 'Pixel 8')).toBe(true)
  })
})

describe('замок', () => {
  it('гасит связь, но не забывает, что ещё не уехало', async () => {
    await core.createRecord({
      service_name: 'Fastmail',
      urls: ['app.fastmail.com'],
      login: 'demo@fastmail.example',
      password: 'mock-fastmail-pw',
    })
    core.control.peerFound(MOCK_DEVICE_PHONE)

    core.control.forceLock('timeout')
    await core.unlock('syncra-dev')

    const status = await core.getSyncStatus()
    expect(status.peers_online).toBe(0)
    expect(status.phase).toBe('idle')
    expect(status.pending_records).toHaveLength(1)
  })
})

describe('демо-цикл (только для дева)', () => {
  it('по умолчанию выключен: тесты не должны менять состояние сами', async () => {
    vi.useFakeTimers()
    const quiet = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    await vi.advanceTimersByTimeAsync(60_000)
    expect((await quiet.getSyncStatus()).phase).toBe('idle')

    quiet.control.dispose()
    vi.useRealTimers()
  })

  it('включённый — проводит хранилище через поиск, обмен и тишину', async () => {
    vi.useFakeTimers()
    const lively = createMockCoreClient({
      latencyMs: 0,
      startUnlocked: true,
      simulateSync: true,
    })
    const phases: string[] = []
    lively.on('sync_status', (status) => phases.push(status.phase))

    await vi.advanceTimersByTimeAsync(4 * 5_000)

    expect(phases).toContain('searching')
    expect(phases).toContain('syncing')
    expect((await lively.getSyncStatus()).last_sync_at).not.toBeNull()

    lively.control.dispose()
    vi.useRealTimers()
  })
})
