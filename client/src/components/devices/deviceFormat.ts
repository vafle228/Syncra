import { formatDate } from '@/components/records/recordFormat'
import { plural, pluralize } from '@/composables/plural'
import type { Device, DeviceKind } from '@/core/contract'

/**
 * Подписи в списке доверенных устройств (F9, §2.3). Чистые функции: только
 * метаданные `Device`, ни одной команды в ядро и ни одного секрета.
 */

/**
 * С какого срока молчания устройство помечается «давно не появлялся».
 *
 * Порог — не про надёжность, а про внимание: пропавший месяц назад ноутбук
 * стоит заметить, а телефон, пролежавший выходные в сумке, — обычное дело.
 * Пометка ни на что не влияет и ничего не запрещает: решение об отзыве
 * принимает человек (§2.3).
 */
export const DEVICE_STALE_DAYS = 30

const MINUTE_MS = 60 * 1000
const HOUR_MS = 60 * MINUTE_MS
const DAY_MS = 24 * HOUR_MS

const MINUTE_FORMS: [string, string, string] = ['минуту', 'минуты', 'минут']
const HOUR_FORMS: [string, string, string] = ['час', 'часа', 'часов']
const DAY_FORMS: [string, string, string] = ['день', 'дня', 'дней']
export const DEVICE_FORMS: [string, string, string] = ['устройство', 'устройства', 'устройств']

export function deviceKindLabel(kind: DeviceKind): string {
  return kind === 'mobile' ? 'Телефон' : 'Компьютер'
}

/** Сколько дней устройство молчит. `null` — не выходило на связь ни разу. */
export function daysSilent(device: Device, now: Date = new Date()): number | null {
  if (device.last_seen_at === null) return null
  const seen = Date.parse(device.last_seen_at)
  if (Number.isNaN(seen)) return null
  return Math.floor((now.getTime() - seen) / DAY_MS)
}

/**
 * Давно ли устройство пропало. Отозванное не считается пропавшим: оно молчит
 * потому, что его отрезали, и говорить об этом второй раз незачем.
 */
export function isStale(device: Device, now: Date = new Date()): boolean {
  if (device.revoked_at !== null || device.is_this_device) return false
  const days = daysSilent(device, now)
  return days === null || days >= DEVICE_STALE_DAYS
}

/** «только что» · «6 минут назад» · «3 часа назад» · «41 день назад». */
export function formatAgo(iso: string, now: Date = new Date()): string {
  const at = Date.parse(iso)
  if (Number.isNaN(at)) return 'неизвестно когда'

  const elapsed = Math.max(0, now.getTime() - at)
  if (elapsed < MINUTE_MS) return 'только что'

  if (elapsed < HOUR_MS) {
    const minutes = Math.floor(elapsed / MINUTE_MS)
    return `${minutes} ${plural(minutes, MINUTE_FORMS)} назад`
  }

  if (elapsed < DAY_MS) {
    const hours = Math.floor(elapsed / HOUR_MS)
    return `${hours} ${plural(hours, HOUR_FORMS)} назад`
  }

  const days = Math.floor(elapsed / DAY_MS)
  return `${days} ${plural(days, DAY_FORMS)} назад`
}

/**
 * Строка под именем устройства.
 *
 * У отозванного она полностью замещает обычную: «когда виделись» перестало быть
 * важным в тот момент, когда доступ отрезали, — важно, что копия на нём больше
 * не обновляется (§2.3).
 */
export function deviceSubtitle(device: Device, now: Date = new Date()): string {
  if (device.revoked_at !== null) {
    return `доступ отозван ${formatDate(device.revoked_at)} · копия на устройстве больше не обновляется`
  }

  const paired = `сопряжено ${formatDate(device.paired_at)}`
  const kind = deviceKindLabel(device.kind)

  if (device.is_this_device) return `${kind} · ${paired} · вы работаете здесь`

  const seen = device.last_seen_at
  const days = daysSilent(device, now)
  if (seen === null || days === null) return `${kind} · ${paired} · ни разу не выходил на связь`
  if (days >= DEVICE_STALE_DAYS) {
    return `${kind} · ${paired} · не появлялся ${pluralize(days, DAY_FORMS)}`
  }

  return `${kind} · ${paired} · был рядом ${formatAgo(seen, now)}`
}
