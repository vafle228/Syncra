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

const WEEK_FORMS: [string, string, string] = ['неделю', 'недели', 'недель']
const MONTH_FORMS: [string, string, string] = ['месяц', 'месяца', 'месяцев']

/** «3 дня» · «3 недели» · «4 месяца» — крупная мера для крупного молчания. */
function gapText(days: number): string {
  if (days < 14) return pluralize(days, DAY_FORMS)
  if (days < 60) return pluralize(Math.floor(days / 7), WEEK_FORMS)
  return pluralize(Math.floor(days / 30), MONTH_FORMS)
}

/** Часы и минуты последнего сеанса связи — «12:04». */
function clock(iso: string): string {
  const at = new Date(iso)
  if (Number.isNaN(at.getTime())) return 'недавно'
  return `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`
}

/**
 * Присутствие устройства — то, что стоит справа в строке (`Прототип:2554-2558`).
 *
 * Макет не печатает дат: человеку важно не «когда именно», а «рядом ли оно
 * сейчас». Отсюда словарь из четырёх состояний — «это устройство», «рядом ·
 * 12:04», «рядом · только что», «не в сети 3 недели» — и точка, цветом
 * повторяющая тот же ответ: акцент, пока связь живая, и серая, когда нет.
 */
export function devicePresence(
  device: Device,
  now: Date = new Date(),
): { text: string; live: boolean } {
  if (device.revoked_at !== null) {
    return { text: `доступ отозван ${formatDate(device.revoked_at)}`, live: false }
  }
  if (device.is_this_device) return { text: 'это устройство', live: true }

  const seen = device.last_seen_at
  const days = daysSilent(device, now)
  if (seen === null || days === null) return { text: 'ни разу не выходило на связь', live: false }

  const elapsed = now.getTime() - Date.parse(seen)
  if (elapsed < MINUTE_MS) return { text: 'рядом · только что', live: true }
  if (elapsed < DAY_MS) return { text: `рядом · ${clock(seen)}`, live: true }

  return { text: `не в сети ${gapText(days)}`, live: false }
}

/**
 * Строка под именем устройства: чем оно является и когда его завели.
 *
 * «Когда виделись» здесь больше НЕТ — это ответ на другой вопрос, и он стоит
 * справа в строке (`devicePresence`). У отозванного вместо всего этого — то
 * единственное, что теперь про него важно: копия на нём больше не обновляется
 * (§2.3).
 */
export function deviceSubtitle(device: Device): string {
  if (device.revoked_at !== null) {
    return `доступ отозван ${formatDate(device.revoked_at)} · копия на устройстве больше не обновляется`
  }

  const paired = `сопряжено ${formatDate(device.paired_at)}`
  return `${deviceKindLabel(device.kind)} · ${paired}`
}

/** Отпечаток под именем: «сокол · медь · январь · парус» (`Прототип:2056`). */
export function fingerprintLine(device: Device): string {
  return device.fingerprint_words.join(' · ')
}
