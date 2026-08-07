/**
 * Русские числительные: «1 запись · 2 записи · 5 записей».
 *
 * Отдельный модуль, потому что счётчики есть и над списком записей, и в
 * карточке секции: две копии этих правил разошлись бы при первой же правке.
 */

/** `forms` — формы для 1, 2–4 и 5+ соответственно. */
export function plural(count: number, forms: [string, string, string]): string {
  const mod100 = Math.abs(count) % 100
  const mod10 = Math.abs(count) % 10
  if (mod100 >= 11 && mod100 <= 14) return forms[2]
  if (mod10 === 1) return forms[0]
  if (mod10 >= 2 && mod10 <= 4) return forms[1]
  return forms[2]
}

/** То же, но вместе с числом: `3 записи`. */
export function pluralize(count: number, forms: [string, string, string]): string {
  return `${count} ${plural(count, forms)}`
}

export const RECORD_FORMS: [string, string, string] = ['запись', 'записи', 'записей']
export const SERVICE_FORMS: [string, string, string] = ['сервис', 'сервиса', 'сервисов']
export const ACCOUNT_FORMS: [string, string, string] = ['аккаунт', 'аккаунта', 'аккаунтов']
