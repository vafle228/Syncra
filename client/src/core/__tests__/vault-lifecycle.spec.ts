// @vitest-environment node
import { describe, expect, it } from 'vitest'

import { MASTER_PASSWORD_MIN_LENGTH, type CoreErrorCode } from '../contract'
import { isCoreError } from '../errors'
import { createMockCoreClient, MOCK_MASTER_PASSWORD, MOCK_PIN_ATTEMPTS } from '../mock'

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
      // Быстрого входа нет, пока его не завели: считать попытки нечего.
      pin: { enrolled: false, attempts_left: null },
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
      pin: { enrolled: false, attempts_left: null },
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

describe('быстрый вход по PIN (F13)', () => {
  it('не предлагается, пока не заведён', async () => {
    const core = createMockCoreClient({ latencyMs: 0 })

    expect(await core.getVaultStatus()).toMatchObject({
      pin: { enrolled: false, attempts_left: null },
    })
    // Команда есть, а PIN нет — это ошибка вызывающего, не «не подошёл».
    await expectCoreError(core.unlockWithPin('1234'), 'VALIDATION')
  })

  it('открывает хранилище верным PIN', async () => {
    const core = createMockCoreClient({ latencyMs: 0, pin: '1234' })

    expect(await core.getVaultStatus()).toMatchObject({
      pin: { enrolled: true, attempts_left: MOCK_PIN_ATTEMPTS },
    })

    const result = await core.unlockWithPin('1234')

    expect(result).toEqual({ ok: true, unlocked_at: expect.any(String) })
    expect(core.control.isUnlocked()).toBe(true)
  })

  it('неверный PIN — не ошибка, а ответ со счётчиком попыток', async () => {
    // Опечатка на четырёх кнопках это ожидаемый исход: обрабатывать её через
    // catch значило бы путать ошибку человека с ошибкой программы.
    const core = createMockCoreClient({ latencyMs: 0, pin: '1234' })

    expect(await core.unlockWithPin('9999')).toEqual({
      ok: false,
      attempts_left: MOCK_PIN_ATTEMPTS - 1,
      pin_disabled: false,
    })
    expect(core.control.isUnlocked()).toBe(false)
    expect(await core.getVaultStatus()).toMatchObject({
      pin: { enrolled: true, attempts_left: MOCK_PIN_ATTEMPTS - 1 },
    })
  })

  it('после исчерпания попыток быстрый вход выключается совсем', async () => {
    const core = createMockCoreClient({ latencyMs: 0, pin: '1234' })

    for (let attempt = 1; attempt < MOCK_PIN_ATTEMPTS; attempt += 1) {
      await core.unlockWithPin('9999')
    }

    expect(await core.unlockWithPin('9999')).toEqual({
      ok: false,
      attempts_left: 0,
      pin_disabled: true,
    })
    // Дальше только мастер-пароль — и даже верный PIN больше не подойдёт.
    expect(await core.getVaultStatus()).toMatchObject({
      pin: { enrolled: false, attempts_left: null },
    })
    await expectCoreError(core.unlockWithPin('1234'), 'VALIDATION')
    expect(core.control.isUnlocked()).toBe(false)
  })

  it('мастер-пароль возвращает попытки: за забытый PIN не наказывают', async () => {
    const core = createMockCoreClient({ latencyMs: 0, pin: '1234' })
    await core.unlockWithPin('9999')
    await core.unlockWithPin('8888')

    await core.unlock(MOCK_MASTER_PASSWORD)

    expect(await core.getVaultStatus()).toMatchObject({
      pin: { enrolled: true, attempts_left: MOCK_PIN_ATTEMPTS },
    })
  })

  it('требует ровно PIN_LENGTH цифр', async () => {
    const core = createMockCoreClient({ latencyMs: 0, pin: '1234' })

    await expectCoreError(core.unlockWithPin('123'), 'VALIDATION')
    await expectCoreError(core.unlockWithPin('12345'), 'VALIDATION')
    await expectCoreError(core.unlockWithPin('12a4'), 'VALIDATION')
    // Ни одна из этих попыток не считается: счётчик тратится на догадки, а не
    // на криво собранный запрос.
    expect(await core.getVaultStatus()).toMatchObject({
      pin: { enrolled: true, attempts_left: MOCK_PIN_ATTEMPTS },
    })
  })

  it('PIN не заводится сам на новом хранилище', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false, pin: '1234' })

    await core.initVault('рыжий трамвай у моста')

    expect(await core.getVaultStatus()).toMatchObject({
      pin: { enrolled: false, attempts_left: null },
    })
  })
})

describe('смена мастер-пароля (F13)', () => {
  const NEXT = 'сосновая шишка на полке'

  it('меняет пароль и оставляет хранилище открытым', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    const result = await core.changeMasterPassword(MOCK_MASTER_PASSWORD, NEXT)

    expect(result.changed_at).toEqual(expect.any(String))
    // Старый пароль только что подтвердили — запирать за это незачем.
    expect(core.control.isUnlocked()).toBe(true)
  })

  it('считает устройства, которые спросят новый пароль', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    const others = (await core.listDevices()).filter(
      (device) => !device.is_this_device && device.revoked_at === null,
    )

    const result = await core.changeMasterPassword(MOCK_MASTER_PASSWORD, NEXT)

    expect(result.devices_to_update).toBe(others.length)
  })

  it('новый пароль действительно становится единственным', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    await core.changeMasterPassword(MOCK_MASTER_PASSWORD, NEXT)

    // Экспорт спрашивает мастер-пароль отдельно — на нём и проверяем.
    await expectCoreError(core.exportCsv(MOCK_MASTER_PASSWORD), 'INVALID_MASTER_PASSWORD')
    await expect(core.exportCsv(NEXT)).resolves.toMatchObject({ encrypted: false })

    await core.lock()
    await expectCoreError(core.unlock(MOCK_MASTER_PASSWORD), 'INVALID_MASTER_PASSWORD')
    await expect(core.unlock(NEXT)).resolves.toMatchObject({ unlocked_at: expect.any(String) })
  })

  it('не верит на слово: неверный текущий пароль ничего не меняет', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    await expectCoreError(
      core.changeMasterPassword('не тот пароль вовсе', NEXT),
      'INVALID_MASTER_PASSWORD',
    )
    await expect(core.exportCsv(MOCK_MASTER_PASSWORD)).resolves.toMatchObject({ encrypted: false })
  })

  it('отклоняет слишком короткий и совпадающий с текущим', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    await expectCoreError(
      core.changeMasterPassword(MOCK_MASTER_PASSWORD, 'a'.repeat(MASTER_PASSWORD_MIN_LENGTH - 1)),
      'VALIDATION',
    )
    await expectCoreError(
      core.changeMasterPassword(MOCK_MASTER_PASSWORD, MOCK_MASTER_PASSWORD),
      'VALIDATION',
    )
  })

  it('за замком: перешифровать можно только открытое хранилище', async () => {
    const core = createMockCoreClient({ latencyMs: 0 })

    await expectCoreError(core.changeMasterPassword(MOCK_MASTER_PASSWORD, NEXT), 'LOCKED')
  })

  it('сбрасывает PIN: он отпирал ключ от старого пароля', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true, pin: '1234' })

    await core.changeMasterPassword(MOCK_MASTER_PASSWORD, NEXT)

    expect(await core.getVaultStatus()).toMatchObject({
      pin: { enrolled: false, attempts_left: null },
    })
  })
})
