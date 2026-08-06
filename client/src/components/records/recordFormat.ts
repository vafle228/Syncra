import type { IsoDateTime } from '@/core/contract'

/** Форматирование метаданных записи для карточки (F5). Секретов не касается. */

const DATE = new Intl.DateTimeFormat('ru-RU', {
  day: '2-digit',
  month: 'short',
  year: 'numeric',
})

export function formatDate(value: IsoDateTime): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '—' : DATE.format(date)
}

const DAY_MS = 24 * 60 * 60 * 1000

export function daysSince(value: IsoDateTime, now: Date = new Date()): number | null {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return null
  return Math.floor((now.getTime() - date.getTime()) / DAY_MS)
}

/**
 * Гигиена пароля (§4.1: `password_updated_at` заведён отдельно ровно ради
 * этого). Предупреждаем спокойно и только там, где счёт пошёл на годы:
 * ежемесячная ротация — миф, а вот пароль возрастом «с прошлого места работы»
 * стоит заменить.
 */
export function passwordAgeWarning(value: IsoDateTime, now: Date = new Date()): string | null {
  const days = daysSince(value, now)
  if (days === null || days < 365) return null

  const years = Math.floor(days / 365)
  return years >= 2
    ? `Пароль не менялся больше ${years} лет — стоит заменить.`
    : 'Пароль не менялся больше года — стоит заменить.'
}
