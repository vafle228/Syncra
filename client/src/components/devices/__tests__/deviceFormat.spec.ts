// @vitest-environment node
import { describe, expect, it } from 'vitest'

import type { Device } from '@/core/contract'
import {
  DEVICE_STALE_DAYS,
  daysSilent,
  deviceKindLabel,
  deviceSubtitle,
  formatAgo,
  isStale,
} from '../deviceFormat'

const NOW = new Date('2026-08-08T12:00:00.000Z')
const MINUTE = 60 * 1000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

function ago(ms: number): string {
  return new Date(NOW.getTime() - ms).toISOString()
}

function device(patch: Partial<Device> = {}): Device {
  return {
    device_id: 'device-x',
    name: 'iPhone 14',
    kind: 'mobile',
    is_this_device: false,
    paired_at: ago(400 * DAY),
    last_seen_at: ago(6 * MINUTE),
    revoked_at: null,
    ...patch,
  }
}

describe('«когда виделись»', () => {
  it('склоняет по-русски и не врёт про точность', () => {
    expect(formatAgo(ago(20 * 1000), NOW)).toBe('только что')
    expect(formatAgo(ago(MINUTE), NOW)).toBe('1 минуту назад')
    expect(formatAgo(ago(6 * MINUTE), NOW)).toBe('6 минут назад')
    expect(formatAgo(ago(2 * HOUR), NOW)).toBe('2 часа назад')
    expect(formatAgo(ago(41 * DAY), NOW)).toBe('41 день назад')
    expect(formatAgo(ago(5 * DAY), NOW)).toBe('5 дней назад')
  })

  it('не уходит в будущее, если часы устройств разошлись', () => {
    expect(formatAgo(new Date(NOW.getTime() + HOUR).toISOString(), NOW)).toBe('только что')
  })

  it('на нечитаемой дате не выдумывает срок', () => {
    expect(formatAgo('позавчера', NOW)).toBe('неизвестно когда')
  })
})

describe('давно пропавшее устройство', () => {
  it('считает молчание в днях', () => {
    expect(daysSilent(device({ last_seen_at: ago(41 * DAY) }), NOW)).toBe(41)
    expect(daysSilent(device({ last_seen_at: null }), NOW)).toBeNull()
  })

  it('помечает то, что молчит дольше порога', () => {
    expect(isStale(device({ last_seen_at: ago(DEVICE_STALE_DAYS * DAY) }), NOW)).toBe(true)
    expect(isStale(device({ last_seen_at: ago(2 * DAY) }), NOW)).toBe(false)
    // Ни разу не выходило на связь — тот же повод присмотреться.
    expect(isStale(device({ last_seen_at: null }), NOW)).toBe(true)
  })

  it('не помечает отозванное: оно молчит потому, что его отрезали', () => {
    const revoked = device({ last_seen_at: ago(90 * DAY), revoked_at: ago(80 * DAY) })

    expect(isStale(revoked, NOW)).toBe(false)
  })

  it('не помечает это устройство: оно здесь по определению', () => {
    expect(isStale(device({ is_this_device: true, last_seen_at: null }), NOW)).toBe(false)
  })
})

describe('подпись под именем устройства', () => {
  it('у своего устройства говорит, что вы на нём и работаете', () => {
    const line = deviceSubtitle(device({ kind: 'desktop', is_this_device: true }), NOW)

    expect(line).toContain('Компьютер')
    expect(line).toContain('вы работаете здесь')
  })

  it('у обычного показывает, когда оно последний раз было рядом', () => {
    expect(deviceSubtitle(device(), NOW)).toContain('был рядом 6 минут назад')
  })

  it('у пропавшего считает срок в днях — так тревожнее и точнее', () => {
    expect(deviceSubtitle(device({ last_seen_at: ago(41 * DAY) }), NOW)).toContain(
      'не появлялся 41 день',
    )
  })

  it('у ни разу не выходившего на связь так и говорит', () => {
    expect(deviceSubtitle(device({ last_seen_at: null }), NOW)).toContain(
      'ни разу не выходил на связь',
    )
  })

  it('у отозванного замещает всё: важно, что копия больше не обновляется (§2.3)', () => {
    const line = deviceSubtitle(device({ revoked_at: ago(DAY) }), NOW)

    expect(line).toContain('доступ отозван')
    expect(line).toContain('копия на устройстве больше не обновляется')
    expect(line).not.toContain('был рядом')
  })

  it('называет тип устройства словом, а не техническим kind', () => {
    expect(deviceKindLabel('mobile')).toBe('Телефон')
    expect(deviceKindLabel('desktop')).toBe('Компьютер')
  })
})
