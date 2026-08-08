// @vitest-environment node
import { beforeEach, describe, expect, it } from 'vitest'

import type { CoreErrorCode } from '../contract'
import { isCoreError } from '../errors'
import {
  createMockCoreClient,
  MOCK_DEVICE_LAPTOP,
  MOCK_DEVICE_PHONE,
  MOCK_MASTER_PASSWORD,
  type MockCoreClient,
} from '../mock'

/**
 * Доверенные устройства и отзыв в фейк-ядре (F9, §2.3).
 *
 * Проверяем обещания, на которые опирается экран: список отдаётся только за
 * замком, отозванное устройство из него не исчезает, себя отозвать нельзя, а
 * момент отзыва — факт, который повторным нажатием не переписывается.
 */

async function expectCoreError(promise: Promise<unknown>, code: CoreErrorCode): Promise<void> {
  let thrown: unknown = null
  let resolved = false

  try {
    await promise
    resolved = true
  } catch (error) {
    thrown = error
  }

  expect(resolved, `ожидалась ошибка ядра ${code}, но промис зарезолвился`).toBe(false)
  expect(isCoreError(thrown, code), `ожидалась ошибка ядра ${code}, получено: ${thrown}`).toBe(true)
}

const FOREIGN_CODE = '4TQ9MB'

let core: MockCoreClient

beforeEach(async () => {
  core = createMockCoreClient({ latencyMs: 0 })
  await core.unlock(MOCK_MASTER_PASSWORD)
})

describe('список устройств', () => {
  it('отдаёт это устройство и сопряжённые с ним', async () => {
    const devices = await core.listDevices()

    expect(devices.filter((device) => device.is_this_device)).toHaveLength(1)
    expect(devices.map((device) => device.device_id)).toContain(MOCK_DEVICE_PHONE)
    expect(devices.every((device) => device.revoked_at === null)).toBe(true)
  })

  it('ЗАКОН №1: ключей и сетевых адресов в списке нет (§2.1)', async () => {
    const [device] = await core.listDevices()

    expect(Object.keys(device ?? {}).sort()).toEqual([
      'device_id',
      'is_this_device',
      'kind',
      'last_seen_at',
      'name',
      'paired_at',
      'revoked_at',
    ])
  })

  it('на закрытом хранилище списка нет: это тоже его содержимое', async () => {
    const locked = createMockCoreClient({ latencyMs: 0 })

    await expectCoreError(locked.listDevices(), 'LOCKED')
    await expectCoreError(locked.revokeDevice(MOCK_DEVICE_PHONE), 'LOCKED')
  })

  it('свежесозданное хранилище знает ровно одно устройство — это', async () => {
    const fresh = createMockCoreClient({ latencyMs: 0, initialized: false })
    await fresh.initVault('достаточно-длинный')

    const devices = await fresh.listDevices()

    expect(devices).toHaveLength(1)
    expect(devices[0]?.is_this_device).toBe(true)
  })

  it('сопряжённое устройство появляется в списке', async () => {
    const before = (await core.listDevices()).length
    const handshake = await core.submitPairedKey(FOREIGN_CODE)
    const { device } = await core.confirmPairing(handshake.session_id)

    const devices = await core.listDevices()

    expect(devices).toHaveLength(before + 1)
    expect(devices.map((item) => item.device_id)).toContain(device.device_id)
  })

  it('замок очищает список только для UI — ядро его помнит', async () => {
    core.control.forceLock('timeout')
    await core.unlock(MOCK_MASTER_PASSWORD)

    expect((await core.listDevices()).length).toBeGreaterThan(1)
  })
})

describe('отзыв доступа (§2.3)', () => {
  it('помечает устройство отозванным, не выкидывая его из списка', async () => {
    const revoked = await core.revokeDevice(MOCK_DEVICE_LAPTOP)

    expect(revoked.revoked_at).not.toBeNull()

    // Отзыв — факт истории хранилища, а не удаление строки: человек должен
    // видеть, что устройство отозвано, а не то, что его никогда не было.
    const devices = await core.listDevices()
    expect(devices.map((device) => device.device_id)).toContain(MOCK_DEVICE_LAPTOP)
    expect(devices.filter((device) => device.revoked_at !== null)).toHaveLength(1)
  })

  it('не трогает соседние устройства', async () => {
    await core.revokeDevice(MOCK_DEVICE_LAPTOP)

    const phone = (await core.listDevices()).find(
      (device) => device.device_id === MOCK_DEVICE_PHONE,
    )
    expect(phone?.revoked_at).toBeNull()
  })

  it('себя отозвать нельзя: это единственное устройство в руках у владельца', async () => {
    const self = (await core.listDevices()).find((device) => device.is_this_device)

    await expectCoreError(core.revokeDevice(self?.device_id ?? ''), 'VALIDATION')
    expect((await core.listDevices()).every((device) => device.revoked_at === null)).toBe(true)
  })

  it('незнакомое устройство — NOT_FOUND, а не молчаливый успех', async () => {
    await expectCoreError(core.revokeDevice('нет такого'), 'NOT_FOUND')
  })

  it('повторный отзыв идемпотентен и не переписывает момент отзыва', async () => {
    let clock = new Date('2026-08-08T10:00:00.000Z')
    const fixed = createMockCoreClient({ latencyMs: 0, now: () => clock })
    await fixed.unlock(MOCK_MASTER_PASSWORD)

    const first = await fixed.revokeDevice(MOCK_DEVICE_LAPTOP)
    clock = new Date(clock.getTime() + 60 * 60 * 1000)
    const second = await fixed.revokeDevice(MOCK_DEVICE_LAPTOP)

    expect(second.revoked_at).toBe(first.revoked_at)
  })

  it('отзыв переживает блокировку: он не «до конца сеанса»', async () => {
    await core.revokeDevice(MOCK_DEVICE_LAPTOP)
    core.control.forceLock('manual')
    await core.unlock(MOCK_MASTER_PASSWORD)

    const laptop = (await core.listDevices()).find(
      (device) => device.device_id === MOCK_DEVICE_LAPTOP,
    )
    expect(laptop?.revoked_at).not.toBeNull()
  })

  it('вернуть отозванное устройство можно только новым сопряжением', async () => {
    await core.revokeDevice(MOCK_DEVICE_LAPTOP)

    // Команды «отменить отзыв» нет намеренно: устройство возвращается через
    // обмен ключами, иначе отзыв не значил бы ничего (§2.2).
    const handshake = await core.submitPairedKey(FOREIGN_CODE)
    const { device } = await core.confirmPairing(handshake.session_id)

    expect(device.device_id).not.toBe(MOCK_DEVICE_LAPTOP)
    expect(device.revoked_at).toBeNull()
  })
})
