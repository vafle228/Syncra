import { describe, expect, it } from 'vitest'

import type { SyncStatus } from '@/core/contract'

import { describeSync, lastSyncLine } from '../syncFormat'

const NOW = new Date('2026-08-08T12:00:00.000Z')

function status(patch: Partial<SyncStatus> = {}): SyncStatus {
  return {
    phase: 'idle',
    peers_online: 0,
    peer_name: null,
    pending_records: [],
    last_sync_at: null,
    message: null,
    ...patch,
  }
}

describe('приоритет состояний', () => {
  it('конфликт важнее всего: он единственный ждёт человека', () => {
    const view = describeSync(
      status({ phase: 'syncing', peer_name: 'iPhone 14', peers_online: 1 }),
      1,
      NOW,
    )

    expect(view.look).toBe('conflict')
    expect(view.tone).toBe('danger')
    expect(view.chip).toBe('1 конфликт')
  })

  it('обрыв важнее счётчиков, но остаётся жёлтым: данные целы', () => {
    const view = describeSync(
      status({ phase: 'error', peer_name: 'iPhone 14', pending_records: ['a'] }),
      0,
      NOW,
    )

    expect(view.look).toBe('error')
    expect(view.tone).toBe('warn')
    expect(view.title).toContain('iPhone 14')
    expect(view.body).toContain('Данные целы')
  })

  it('обмен пульсирует и называет собеседника', () => {
    const view = describeSync(status({ phase: 'syncing', peer_name: 'iPhone 14' }), 0, NOW)

    expect(view.look).toBe('syncing')
    expect(view.pulse).toBe(true)
    expect(view.chip).toBe('Обмен с iPhone 14')
  })

  it('поиск пиров подписан честно, а выглядит как обмен', () => {
    const view = describeSync(status({ phase: 'searching' }), 0, NOW)

    expect(view.look).toBe('searching')
    expect(view.tone).toBe('accent')
    expect(view.pulse).toBe(true)
    expect(view.chip).toBe('Ищу устройства')
  })
})

describe('тишина', () => {
  it('ожидающие изменения — жёлтые, но без требования что-то делать', () => {
    const view = describeSync(status({ pending_records: ['a', 'b'] }), 0, NOW)

    expect(view.look).toBe('pending')
    expect(view.tone).toBe('warn')
    expect(view.chip).toBe('2 изменения ждут')
    expect(view.body).toContain('уедут сами')
  })

  it('одно изменение склоняется в единственном числе', () => {
    expect(describeSync(status({ pending_records: ['a'] }), 0, NOW).chip).toBe('1 изменение ждёт')
  })

  it('«рядом никого» — серый покой, а не ошибка', () => {
    const view = describeSync(status(), 0, NOW)

    expect(view.look).toBe('alone')
    expect(view.tone).toBe('calm')
    expect(view.body).toContain('не «сломалась»')
  })

  it('всё сошлось — акцент и время последнего обмена', () => {
    const view = describeSync(
      status({ peers_online: 2, last_sync_at: '2026-08-08T11:58:00.000Z' }),
      0,
      NOW,
    )

    expect(view.look).toBe('ok')
    expect(view.tone).toBe('accent')
    expect(view.chip).toBe('Синхронизировано')
    expect(view.body).toContain('2 устройства')
    expect(view.body).toContain('2 минуты назад')
  })
})

describe('время последнего обмена', () => {
  it('честно говорит, что обмена ещё не было', () => {
    expect(lastSyncLine(status(), NOW)).toBe('обмена ещё не было')
  })

  it('считает от текущего времени', () => {
    expect(lastSyncLine(status({ last_sync_at: '2026-08-08T09:00:00.000Z' }), NOW)).toBe(
      'последний обмен — 3 часа назад',
    )
  })
})
