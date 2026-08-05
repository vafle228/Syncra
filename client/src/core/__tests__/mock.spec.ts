// @vitest-environment node
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { CoreErrorCode, RecordMeta } from '../contract'
import { CoreError, isCoreError } from '../errors'
import { createMockCoreClient, MOCK_MASTER_PASSWORD, type MockCoreClient } from '../mock'

/** Ключи, которых не должно быть в метаданных ни при каких обстоятельствах. */
const SECRET_KEYS = ['password', 'notes', 'totp_secret'] as const

function expectNoSecrets(value: object): void {
  const serialized = JSON.stringify(value)
  for (const key of SECRET_KEYS) {
    expect(Object.keys(value)).not.toContain(key)
    // Вложенные объекты тоже: ищем именно ключ, а не подстроку
    // (`password_updated_at` — легитимные метаданные).
    expect(serialized).not.toMatch(new RegExp(`"${key}"\\s*:`))
  }
  // Ни одно значение сид-секретов не должно просочиться под другим именем.
  expect(serialized).not.toMatch(/mock-[a-z]+-pw/)
  expect(serialized).not.toContain('MOCKTOTPSECRET')
}

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

let core: MockCoreClient

beforeEach(async () => {
  core = createMockCoreClient({ latencyMs: 0 })
  await core.unlock(MOCK_MASTER_PASSWORD)
})

describe('замок', () => {
  it('стартует заблокированным и не отдаёт данные до unlock', async () => {
    const locked = createMockCoreClient({ latencyMs: 0 })

    expect(locked.control.isUnlocked()).toBe(false)
    await expectCoreError(locked.listRecords(), 'LOCKED')
    await expectCoreError(locked.getSecret('любой-id'), 'LOCKED')
  })

  it('отклоняет неверный мастер-пароль', async () => {
    const locked = createMockCoreClient({ latencyMs: 0 })

    await expectCoreError(locked.unlock('не тот пароль'), 'INVALID_MASTER_PASSWORD')
    expect(locked.control.isUnlocked()).toBe(false)
  })

  it('шлёт события unlocked / locked и снимает подписку', async () => {
    const client = createMockCoreClient({ latencyMs: 0 })
    const onUnlocked = vi.fn()
    const onLocked = vi.fn()

    client.on('unlocked', onUnlocked)
    const stop = client.on('locked', onLocked)

    await client.unlock(MOCK_MASTER_PASSWORD)
    expect(onUnlocked).toHaveBeenCalledTimes(1)

    await client.lock()
    expect(onLocked).toHaveBeenCalledWith(expect.objectContaining({ reason: 'manual' }))
    expect(client.control.isUnlocked()).toBe(false)

    stop()
    await client.unlock(MOCK_MASTER_PASSWORD)
    client.control.forceLock('timeout')
    expect(onLocked).toHaveBeenCalledTimes(1)
  })
})

describe('listRecords', () => {
  it('отдаёт только метаданные — ни одного секретного поля', async () => {
    const records = await core.listRecords()

    expect(records.length).toBeGreaterThan(0)
    for (const record of records) expectNoSecrets(record)
  })

  it('скрывает tombstones, но отдаёт их по явному запросу', async () => {
    const live = await core.listRecords()
    const all = await core.listRecords({ include_deleted: true })

    expect(live.every((r) => r.deleted_at === null)).toBe(true)
    expect(all.length).toBeGreaterThan(live.length)
    expect(all.some((r) => r.deleted_at !== null)).toBe(true)
  })

  it('фильтрует по секции', async () => {
    const all = await core.listRecords()
    const vaultId = all[0]?.vault_id
    expect(vaultId).toBeDefined()

    const scoped = await core.listRecords({ vault_id: vaultId })
    expect(scoped.length).toBeGreaterThan(0)
    expect(scoped.every((r) => r.vault_id === vaultId)).toBe(true)
  })

  it('держит несколько аккаунтов одного сервиса как отдельные записи (§4.4)', async () => {
    const google = (await core.listRecords()).filter((r) => r.service_name === 'Google')

    expect(google).toHaveLength(2)
    expect(new Set(google.map((r) => r.record_id)).size).toBe(2)
    expect(new Set(google.map((r) => r.login)).size).toBe(2)
  })

  it('возвращает копии: правка результата не меняет состояние ядра', async () => {
    const before = await core.listRecords()
    const first = before[0]
    expect(first).toBeDefined()

    first!.service_name = 'ПОДМЕНА'
    first!.urls.push('evil.example')

    const after = await core.listRecords()
    expect(after[0]?.service_name).not.toBe('ПОДМЕНА')
    expect(after[0]?.urls).not.toContain('evil.example')
  })
})

describe('getSecret', () => {
  it('отдаёт секреты только по явному запросу конкретной записи', async () => {
    const records = await core.listRecords()
    const personalGoogle = records.find((r) => r.login === 'personal.demo@gmail.com')
    expect(personalGoogle).toBeDefined()

    const secrets = await core.getSecret(personalGoogle!.record_id)

    expect(secrets.password).toBe('mock-google-personal-pw')
    expect(secrets).toHaveProperty('notes')
    expect(secrets).toHaveProperty('totp_secret')
  })

  it('падает NOT_FOUND на неизвестной записи и на tombstone', async () => {
    const tombstone = (await core.listRecords({ include_deleted: true })).find(
      (r) => r.deleted_at !== null,
    )
    expect(tombstone).toBeDefined()

    await expectCoreError(core.getSecret('нет-такого'), 'NOT_FOUND')
    await expectCoreError(core.getSecret(tombstone!.record_id), 'NOT_FOUND')
  })
})

describe('CRUD', () => {
  function create(): Promise<RecordMeta> {
    return core.createRecord({
      service_name: '  Новый сервис  ',
      urls: ['new.example'],
      login: 'user@new.example',
      password: ' пароль с пробелами ',
      notes: 'заметка',
    })
  }

  it('создаёт запись с version 1 и без секретов в ответе', async () => {
    const created = await create()

    expect(created.version).toBe(1)
    expect(created.deleted_at).toBeNull()
    expect(created.service_name).toBe('Новый сервис')
    expectNoSecrets(created)

    const secrets = await core.getSecret(created.record_id)
    // Пароль сохраняется байт в байт: пробелы по краям не обрезаются.
    expect(secrets.password).toBe(' пароль с пробелами ')
  })

  it('требует обязательные поля', async () => {
    await expectCoreError(
      core.createRecord({ service_name: ' ', urls: [], login: 'a', password: 'b' }),
      'VALIDATION',
    )
    await expectCoreError(
      core.createRecord({ service_name: 'a', urls: [], login: 'a', password: '   ' }),
      'VALIDATION',
    )
  })

  it('инкрементирует version и двигает password_updated_at только при смене пароля', async () => {
    const created = await create()

    const renamed = await core.updateRecord(created.record_id, { service_name: 'Переименован' })
    expect(renamed.version).toBe(2)
    expect(renamed.service_name).toBe('Переименован')
    expect(renamed.password_updated_at).toBe(created.password_updated_at)

    const repassworded = await core.updateRecord(created.record_id, { password: 'другой-пароль' })
    expect(repassworded.version).toBe(3)
    expect(Date.parse(repassworded.password_updated_at)).toBeGreaterThanOrEqual(
      Date.parse(created.password_updated_at),
    )

    const secrets = await core.getSecret(created.record_id)
    expect(secrets.password).toBe('другой-пароль')
    // Незатронутые патчем секретные поля сохраняются.
    expect(secrets.notes).toBe('заметка')
  })

  it('удаление ставит tombstone и стирает секреты', async () => {
    const created = await create()
    const tombstone = await core.deleteRecord(created.record_id)

    expect(tombstone.deleted_at).not.toBeNull()
    expect(tombstone.version).toBe(created.version + 1)

    const live = await core.listRecords()
    expect(live.map((r) => r.record_id)).not.toContain(created.record_id)

    await expectCoreError(core.getSecret(created.record_id), 'NOT_FOUND')
    await expectCoreError(core.deleteRecord(created.record_id), 'NOT_FOUND')
  })
})

describe('эмуляция ядра', () => {
  it('впрыскивает ошибку в следующую команду', async () => {
    core.control.failNext('INTERNAL')

    await expect(core.listRecords()).rejects.toBeInstanceOf(CoreError)
    // Ошибка одноразовая — следующая команда проходит.
    await expect(core.listRecords()).resolves.toBeInstanceOf(Array)
  })

  it('эмулирует задержку IPC', async () => {
    const slow = createMockCoreClient({ latencyMs: 40, startUnlocked: true })

    const startedAt = Date.now()
    await slow.listRecords()
    expect(Date.now() - startedAt).toBeGreaterThanOrEqual(30)
  })

  it('reset возвращает сид и снимает разблокировку', async () => {
    const created = await core.createRecord({
      service_name: 'Временный',
      urls: [],
      login: 'x',
      password: 'y',
    })

    core.control.reset()
    expect(core.control.isUnlocked()).toBe(false)

    await core.unlock(MOCK_MASTER_PASSWORD)
    const records = await core.listRecords()
    expect(records.map((r) => r.record_id)).not.toContain(created.record_id)
  })
})
