import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { setCoreClient } from '@/core/ipc'
import {
  createMockCoreClient,
  MOCK_RECORD_GITHUB,
  MOCK_VAULT_PERSONAL,
  type MockConflictEntry,
  type MockCoreClient,
} from '@/core/mock'

import { useConflictsStore } from '../useConflictsStore'
import { useRecordsStore } from '../useRecordsStore'

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

describe('загрузка конфликтов', () => {
  it('забирает список у ядра и находит конфликт по записи', async () => {
    const conflicts = useConflictsStore()

    await conflicts.load()

    expect(conflicts.count).toBe(1)
    expect(conflicts.hasConflicts).toBe(true)
    expect(conflicts.byRecord(MOCK_RECORD_GITHUB)?.remote.device_name).toBe('iPhone 14')
    expect(conflicts.byRecord('нет-такой-записи')).toBeNull()
    expect(conflicts.byRecord(null)).toBeNull()
  })

  it('в состоянии нет ни одного секретного значения (Закон №1)', async () => {
    const conflicts = useConflictsStore()
    await conflicts.load()

    const state = JSON.stringify(conflicts.conflicts)
    expect(state).not.toContain('mock-github-pw')
    expect(state).not.toContain('Recovery codes')
    expect(state).not.toContain('MOCKTOTPSECRET')
  })

  it('ensure() не ходит в ядро повторно', async () => {
    const conflicts = useConflictsStore()
    await conflicts.ensure()

    await core.resolveConflict(MOCK_RECORD_GITHUB, 'local')
    await conflicts.ensure()
    expect(conflicts.count).toBe(1)

    await conflicts.load()
    expect(conflicts.count).toBe(0)
  })

  it('показывает сообщение ядра при отказе', async () => {
    core.control.failNext('INTERNAL', 'Ядро занято.')
    const conflicts = useConflictsStore()

    await conflicts.load()

    expect(conflicts.error).toBe('Ядро занято.')
    expect(conflicts.count).toBe(0)
    expect(conflicts.loaded).toBe(false)
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
      vault_id: MOCK_VAULT_PERSONAL,
      service_name: 'GitHub',
      urls: ['github.com'],
      login: 'demo-user',
      account_label: null,
    },
    secrets: { password: 'mock-github-pw-thinkpad', notes: null, totp_secret: null },
  }

  it('появляется по событию, не заводя второго спора о той же записи', async () => {
    const conflicts = useConflictsStore()
    await conflicts.load()

    core.control.raiseConflict(entry)

    expect(conflicts.count).toBe(1)
    expect(conflicts.byRecord(MOCK_RECORD_GITHUB)?.remote.device_name).toBe('ThinkPad X1')
  })
})

describe('выбор версии', () => {
  it('убирает конфликт и кладёт победившую версию в список записей', async () => {
    const records = useRecordsStore()
    const conflicts = useConflictsStore()
    await records.load()
    await conflicts.load()

    await conflicts.resolve(MOCK_RECORD_GITHUB, 'remote')

    expect(conflicts.count).toBe(0)
    const github = records.records.find((record) => record.record_id === MOCK_RECORD_GITHUB)
    expect(github?.urls).toEqual(['github.com', 'gist.github.com'])
  })

  it('пробрасывает отказ ядра наружу, не гася экран', async () => {
    const conflicts = useConflictsStore()
    await conflicts.load()

    await expect(conflicts.resolve('нет-такой-записи', 'local')).rejects.toMatchObject({
      code: 'NOT_FOUND',
    })
    expect(conflicts.error).toBeNull()
    expect(conflicts.count).toBe(1)
  })
})

describe('замок', () => {
  it('очищает список конфликтов', async () => {
    const conflicts = useConflictsStore()
    await conflicts.load()

    core.control.forceLock('system')

    expect(conflicts.count).toBe(0)
    expect(conflicts.loaded).toBe(false)
  })

  it('после dispose события больше не двигают состояние', async () => {
    const conflicts = useConflictsStore()
    await conflicts.load()

    conflicts.dispose()
    core.control.forceLock('manual')

    expect(conflicts.count).toBe(1)
  })
})
