import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { setCoreClient } from '../ipc'
import {
  createMockCoreClient,
  MOCK_RECORD_GITHUB,
  MOCK_VAULT_WORK,
  type MockConflictEntry,
  type MockCoreClient,
} from '../mock'

/**
 * Конфликты версий в фейк-ядре (F11, §5.5).
 *
 * Главная проверка здесь — не «две колонки нарисовались», а Закон №1: полный
 * diff доезжает до UI, НЕ привозя с собой ни одного секретного значения.
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

describe('список конфликтов', () => {
  it('на закрытом хранилище недоступен', async () => {
    const locked = createMockCoreClient({ latencyMs: 0 })

    await expect(locked.listConflicts()).rejects.toMatchObject({ code: 'LOCKED' })
  })

  it('отдаёт обе версии с датами и именами устройств', async () => {
    const [conflict] = await core.listConflicts()

    expect(conflict?.record_id).toBe(MOCK_RECORD_GITHUB)
    expect(conflict?.local).toMatchObject({ side: 'local', device_name: 'Этот компьютер' })
    expect(conflict?.remote).toMatchObject({ side: 'remote', device_name: 'iPhone 14' })
    // Обе стороны честно досчитали до одного и того же номера — ровно поэтому
    // выбор идёт стороной, а не версией.
    expect(conflict?.local.version).toBe(conflict?.remote.version)
    expect(Date.parse(conflict!.remote.updated_at)).toBeGreaterThan(
      Date.parse(conflict!.local.updated_at),
    )
  })

  it('НЕ привозит секретных значений — только имена разошедшихся полей', async () => {
    const conflicts = await core.listConflicts()
    const wire = JSON.stringify(conflicts)

    expect(wire).not.toContain('mock-github-pw')
    expect(wire).not.toContain('Recovery codes')
    expect(wire).not.toContain('MOCKTOTPSECRET')

    expect(conflicts[0]?.differing_fields).toEqual(['urls', 'password', 'notes'])
    // Ключ TOTP одинаковый в обеих версиях — в diff он попасть не должен.
    expect(conflicts[0]?.differing_fields).not.toContain('totp_secret')
  })

  it('местная сторона строится из живой записи, а не из снимка', async () => {
    await core.updateRecord(MOCK_RECORD_GITHUB, { login: 'demo-user-renamed' })

    const [conflict] = await core.listConflicts()
    expect(conflict?.local.login).toBe('demo-user-renamed')
    expect(conflict?.differing_fields).toContain('login')
  })

  it('удалили запись — спорить не о чем', async () => {
    await core.deleteRecord(MOCK_RECORD_GITHUB)

    expect(await core.listConflicts()).toHaveLength(0)
  })
})

describe('значения секретов при разрешении', () => {
  it('отдаёт одно поле обеих версий — разово, по запросу', async () => {
    const password = await core.getConflictSecret(MOCK_RECORD_GITHUB, 'password')

    expect(password).toEqual({ local: 'mock-github-pw', remote: 'mock-github-pw-phone' })

    const notes = await core.getConflictSecret(MOCK_RECORD_GITHUB, 'notes')
    expect(notes.local).toContain('mock-1111')
    expect(notes.remote).toContain('mock-4444')
  })

  it('на закрытом хранилище и по чужой записи отказывает', async () => {
    const locked = createMockCoreClient({ latencyMs: 0 })
    await expect(locked.getConflictSecret(MOCK_RECORD_GITHUB, 'password')).rejects.toMatchObject({
      code: 'LOCKED',
    })

    const [record] = (await core.listRecords()).filter(
      (item) => item.record_id !== MOCK_RECORD_GITHUB,
    )
    await expect(core.getConflictSecret(record!.record_id, 'password')).rejects.toMatchObject({
      code: 'NOT_FOUND',
    })
  })
})

describe('выбор версии (§5.5)', () => {
  it('оставляет местную версию нетронутой, но поднимает её выше обеих', async () => {
    const before = (await core.listRecords()).find(
      (record) => record.record_id === MOCK_RECORD_GITHUB,
    )!

    const resolved = await core.resolveConflict(MOCK_RECORD_GITHUB, 'local')

    expect(resolved.login).toBe(before.login)
    expect(resolved.urls).toEqual(before.urls)
    expect(resolved.version).toBe(before.version + 1)
    expect((await core.getSecret(MOCK_RECORD_GITHUB)).password).toBe('mock-github-pw')
    expect(await core.listConflicts()).toHaveLength(0)
  })

  it('оставляет приехавшую версию целиком — вместе с её секретами', async () => {
    const resolved = await core.resolveConflict(MOCK_RECORD_GITHUB, 'remote')

    expect(resolved.urls).toEqual(['github.com', 'gist.github.com'])
    const secrets = await core.getSecret(MOCK_RECORD_GITHUB)
    expect(secrets.password).toBe('mock-github-pw-phone')
    expect(secrets.notes).toContain('mock-4444')
    // Пароль сменился — дата его смены тоже (§4.1).
    expect(resolved.password_updated_at).toBe(resolved.updated_at)
  })

  it('проигравшую версию не сохраняет: склеивать по полям продукт не обещал', async () => {
    await core.resolveConflict(MOCK_RECORD_GITHUB, 'local')

    await expect(core.getConflictSecret(MOCK_RECORD_GITHUB, 'password')).rejects.toMatchObject({
      code: 'NOT_FOUND',
    })
    await expect(core.resolveConflict(MOCK_RECORD_GITHUB, 'remote')).rejects.toMatchObject({
      code: 'NOT_FOUND',
    })
  })

  it('выбор должен доехать до второго устройства', async () => {
    await core.resolveConflict(MOCK_RECORD_GITHUB, 'remote')

    expect((await core.getSyncStatus()).pending_records).toEqual([MOCK_RECORD_GITHUB])
  })
})

describe('приехавший конфликт', () => {
  const entry: MockConflictEntry = {
    record_id: MOCK_RECORD_GITHUB,
    raised_at: '2026-04-01T10:00:00.000Z',
    device_name: 'ThinkPad X1',
    version: 9,
    updated_at: '2026-04-01T09:59:00.000Z',
    meta: {
      vault_id: MOCK_VAULT_WORK,
      service_name: 'GitHub',
      urls: ['github.com'],
      login: 'demo-user',
      account_label: 'Рабочий',
    },
    secrets: { password: 'mock-github-pw', notes: null, totp_secret: null },
  }

  it('приходит событием и заменяет прежний спор по той же записи', async () => {
    const raised: string[] = []
    core.on('conflict_raised', (conflict) => raised.push(conflict.remote.device_name))

    core.control.raiseConflict(entry)

    expect(raised).toEqual(['ThinkPad X1'])
    const conflicts = await core.listConflicts()
    expect(conflicts).toHaveLength(1)
    expect(conflicts[0]?.remote.device_name).toBe('ThinkPad X1')
    // Пароли и адреса совпали; разошлись секция, метка, заметки и ключ TOTP
    // (на том устройстве его не заводили).
    expect(conflicts[0]?.differing_fields).toEqual([
      'vault_id',
      'account_label',
      'notes',
      'totp_secret',
    ])
    expect(conflicts[0]?.differing_fields).not.toContain('password')
  })

  it('победившая версия перебивает и приехавший номер (§5.2)', async () => {
    core.control.raiseConflict(entry)

    const resolved = await core.resolveConflict(MOCK_RECORD_GITHUB, 'local')
    expect(resolved.version).toBe(10)
  })
})
