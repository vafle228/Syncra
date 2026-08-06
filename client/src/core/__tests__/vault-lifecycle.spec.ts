// @vitest-environment node
import { describe, expect, it } from 'vitest'

import { MASTER_PASSWORD_MIN_LENGTH, type CoreErrorCode } from '../contract'
import { isCoreError } from '../errors'
import { createMockCoreClient, MOCK_MASTER_PASSWORD } from '../mock'

/** Жизненный цикл хранилища: создание, замок, статус (F3). */

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

describe('getVaultStatus', () => {
  it('отвечает на неинициализированном хранилище', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })

    await expect(core.getVaultStatus()).resolves.toEqual({
      initialized: false,
      unlocked: false,
      unlocked_at: null,
    })
  })

  it('отвечает на заблокированном и отражает разблокировку', async () => {
    const core = createMockCoreClient({ latencyMs: 0 })

    expect(await core.getVaultStatus()).toMatchObject({ initialized: true, unlocked: false })

    const { unlocked_at } = await core.unlock(MOCK_MASTER_PASSWORD)
    expect(await core.getVaultStatus()).toEqual({
      initialized: true,
      unlocked: true,
      unlocked_at,
    })

    await core.lock()
    expect(await core.getVaultStatus()).toMatchObject({ unlocked: false, unlocked_at: null })
  })
})

describe('initVault', () => {
  it('создаёт хранилище, сразу открывает его и шлёт unlocked', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })
    const events: unknown[] = []
    core.on('unlocked', (payload) => events.push(payload))

    const response = await core.initVault('рыжий трамвай у моста')

    expect(response.initialized_at).toEqual(expect.any(String))
    expect(events).toHaveLength(1)
    expect(await core.getVaultStatus()).toMatchObject({ initialized: true, unlocked: true })
  })

  it('оставляет новое хранилище пустым — записи заводит уже пользователь', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })
    await core.initVault('рыжий трамвай у моста')

    await expect(core.listRecords()).resolves.toEqual([])
  })

  it('принимает заданный пароль для последующих входов', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })
    await core.initVault('рыжий трамвай у моста')
    await core.lock()

    await expectCoreError(core.unlock(MOCK_MASTER_PASSWORD), 'INVALID_MASTER_PASSWORD')
    await expect(core.unlock('рыжий трамвай у моста')).resolves.toBeTruthy()
  })

  it('отклоняет слишком короткий мастер-пароль', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })

    await expectCoreError(core.initVault('a'.repeat(MASTER_PASSWORD_MIN_LENGTH - 1)), 'VALIDATION')
    expect(core.control.isInitialized()).toBe(false)
  })

  it('не даёт создать хранилище поверх существующего', async () => {
    const core = createMockCoreClient({ latencyMs: 0 })

    await expectCoreError(core.initVault('другой пароль совсем'), 'ALREADY_INITIALIZED')
  })
})

describe('до инициализации', () => {
  it('unlock и данные падают NOT_INITIALIZED, а не LOCKED', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })

    await expectCoreError(core.unlock(MOCK_MASTER_PASSWORD), 'NOT_INITIALIZED')
    await expectCoreError(core.listRecords(), 'NOT_INITIALIZED')
    await expectCoreError(core.getSecret('любой-id'), 'NOT_INITIALIZED')
  })

  it('startUnlocked не открывает несуществующее хранилище', () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false, startUnlocked: true })

    expect(core.control.isUnlocked()).toBe(false)
  })

  it('reset возвращает мок к «первому запуску»', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })
    await core.initVault('рыжий трамвай у моста')

    core.control.reset()

    expect(core.control.isInitialized()).toBe(false)
    expect(core.control.isUnlocked()).toBe(false)
  })
})
