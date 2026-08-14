// @vitest-environment node
import { describe, expect, it } from 'vitest'

import type { Device } from '@/core/contract'
import {
  DEVICE_STALE_DAYS,
  daysSilent,
  deviceKindLabel,
  devicePresence,
  deviceSubtitle,
  fingerprintLine,
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
    fingerprint_words: ['сокол', 'медь', 'январь', 'парус'],
    last_seen_at: ago(6 * MINUTE),
    revoked_at: null,
    ...patch,
  }
}

describe('присутствие устройства', () => {
  it('у своего устройства не считает время — оно здесь', () => {
    expect(devicePresence(device({ is_this_device: true }), NOW)).toEqual({
      text: 'это устройство',
      live: true,
    })
  })

  it('связь в пределах суток — «рядом», и точка горит', () => {
    expect(devicePresence(device({ last_seen_at: ago(20 * 1000) }), NOW)).toEqual({
      text: 'рядом · только что',
      live: true,
    })

    const recent = devicePresence(device({ last_seen_at: ago(6 * MINUTE) }), NOW)
    expect(recent.text).toMatch(/^рядом · \d{2}:\d{2}$/)
    expect(recent.live).toBe(true)
  })

  it('долгое молчание меряется крупной мерой, а точка гаснет', () => {
    expect(devicePresence(device({ last_seen_at: ago(3 * DAY) }), NOW)).toEqual({
      text: 'не в сети 3 дня',
      live: false,
    })
    expect(devicePresence(device({ last_seen_at: ago(21 * DAY) }), NOW).text).toBe(
      'не в сети 3 недели',
    )
    expect(devicePresence(device({ last_seen_at: ago(120 * DAY) }), NOW).text).toBe(
      'не в сети 4 месяца',
    )
  })

  it('ни разу не выходившее на связь так и говорит', () => {
    expect(devicePresence(device({ last_seen_at: null }), NOW)).toEqual({
      text: 'ни разу не выходило на связь',
      live: false,
    })
  })

  it('у отозванного присутствие говорит про отзыв, а не про связь', () => {
    const presence = devicePresence(device({ revoked_at: ago(DAY) }), NOW)

    expect(presence.text).toContain('доступ отозван')
    expect(presence.live).toBe(false)
  })

  it('отпечаток печатается словами через точку — их и сверяют глазами', () => {
    expect(fingerprintLine(device())).toBe('сокол · медь · январь · парус')
  })
})

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
  it('говорит, что это за устройство и когда его завели', () => {
    const line = deviceSubtitle(device({ kind: 'desktop' }))

    expect(line).toContain('Компьютер')
    expect(line).toContain('сопряжено')
    // «Когда виделись» переехало в присутствие справа — здесь его больше нет.
    expect(line).not.toContain('рядом')
  })

  it('у отозванного замещает всё: важно, что копия больше не обновляется (§2.3)', () => {
    const line = deviceSubtitle(device({ revoked_at: ago(DAY) }))

    expect(line).toContain('доступ отозван')
    expect(line).toContain('копия на устройстве больше не обновляется')
    expect(line).not.toContain('сопряжено')
  })

  it('называет тип устройства словом, а не техническим kind', () => {
    expect(deviceKindLabel('mobile')).toBe('Телефон')
    expect(deviceKindLabel('desktop')).toBe('Компьютер')
  })
})
