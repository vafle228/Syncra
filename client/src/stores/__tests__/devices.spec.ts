import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_DEVICE_LAPTOP, MOCK_DEVICE_PHONE } from '@/core/mock'
import type { MockCoreClient } from '@/core/mock'
import { useDevicesStore } from '../useDevicesStore'

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

describe('загрузка устройств', () => {
  it('забирает список у ядра и находит это устройство', async () => {
    const devices = useDevicesStore()

    await devices.load()

    expect(devices.loaded).toBe(true)
    expect(devices.thisDevice?.is_this_device).toBe(true)
    expect(devices.active).toHaveLength(devices.devices.length)
    expect(devices.revoked).toHaveLength(0)
    expect(devices.isAlone).toBe(false)
  })

  it('ensure() не ходит в ядро повторно', async () => {
    const devices = useDevicesStore()
    await devices.ensure()
    const before = devices.devices.length

    // Устройство отозвано мимо стора: если бы ensure() перезапрашивал,
    // пометка появилась бы и здесь.
    await core.revokeDevice(MOCK_DEVICE_PHONE)
    await devices.ensure()
    expect(devices.revoked).toHaveLength(0)

    await devices.load()
    expect(devices.devices).toHaveLength(before)
    expect(devices.revoked).toHaveLength(1)
  })

  it('показывает сообщение ядра и оставляет список пустым при ошибке', async () => {
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    const devices = useDevicesStore()

    await devices.load()

    expect(devices.error).toBe('Ядро недоступно.')
    expect(devices.devices).toHaveLength(0)
    expect(devices.loaded).toBe(false)
  })

  it('одно действующее устройство — значит копий хранилища больше нет', async () => {
    const alone = createMockCoreClient({ latencyMs: 0, initialized: false })
    setCoreClient(alone)
    await alone.initVault('достаточно-длинный')

    const devices = useDevicesStore()
    await devices.load()

    expect(devices.isAlone).toBe(true)
  })
})

describe('отзыв доступа (§2.3)', () => {
  it('помечает устройство отозванным на месте, не убирая из списка', async () => {
    const devices = useDevicesStore()
    await devices.load()
    const before = devices.devices.length

    await devices.revoke(MOCK_DEVICE_LAPTOP)

    expect(devices.devices).toHaveLength(before)
    expect(devices.revoked.map((device) => device.device_id)).toEqual([MOCK_DEVICE_LAPTOP])
    expect(devices.active.map((device) => device.device_id)).not.toContain(MOCK_DEVICE_LAPTOP)
  })

  it('отзыв доезжает до ядра, а не остаётся в памяти UI', async () => {
    const devices = useDevicesStore()
    await devices.load()

    await devices.revoke(MOCK_DEVICE_LAPTOP)
    await devices.load()

    expect(devices.revoked.map((device) => device.device_id)).toEqual([MOCK_DEVICE_LAPTOP])
  })

  it('пробрасывает отказ ядра наружу, не гася список', async () => {
    const devices = useDevicesStore()
    await devices.load()
    const self = devices.thisDevice

    await expect(devices.revoke(self?.device_id ?? '')).rejects.toMatchObject({
      code: 'VALIDATION',
    })
    expect(devices.error).toBeNull()
    expect(devices.revoked).toHaveLength(0)
  })
})

describe('события синхронизации (F10)', () => {
  it('двигает «был рядом» по событию, не перечитывая список', async () => {
    const devices = useDevicesStore()
    await devices.load()
    const before = devices.devices.find((device) => device.device_id === MOCK_DEVICE_LAPTOP)

    core.control.peerFound(MOCK_DEVICE_LAPTOP)

    const after = devices.devices.find((device) => device.device_id === MOCK_DEVICE_LAPTOP)
    expect(after?.last_seen_at).not.toBe(before?.last_seen_at)
    expect(devices.devices).toHaveLength(before === undefined ? 0 : 3)
  })

  it('узнаёт о сопряжении, которое подтвердили на втором устройстве', async () => {
    const devices = useDevicesStore()
    await devices.load()
    const before = devices.devices.length

    const result = core.control.pairedByPeer('Pixel 8')

    expect(devices.justPaired?.device.name).toBe('Pixel 8')
    expect(devices.justPaired?.records_transferred).toBe(result.records_transferred)
    expect(devices.devices).toHaveLength(before + 1)

    devices.forgetPaired()
    expect(devices.justPaired).toBeNull()
  })
})

describe('замок', () => {
  it('очищает список устройств по событию блокировки', async () => {
    const devices = useDevicesStore()
    await devices.load()
    expect(devices.devices.length).toBeGreaterThan(0)

    core.control.forceLock('timeout')

    expect(devices.devices).toHaveLength(0)
    expect(devices.loaded).toBe(false)
  })

  it('после dispose события больше не двигают состояние', async () => {
    const devices = useDevicesStore()
    await devices.load()
    const before = devices.devices.length

    devices.dispose()
    core.control.forceLock('system')

    expect(devices.devices).toHaveLength(before)
  })
})
