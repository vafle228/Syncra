/**
 * Локальная иконка-заглушка для записи (F2, §3.1 брифа).
 *
 * ПРИВАТНОСТЬ: фавиконы из сети не запрашиваются никогда — такой запрос выдал бы
 * чужому серверу список сервисов пользователя. Плитка рисуется на устройстве:
 * 1–2 буквы из имени + тон из хэша строки по восьми фиксированным ступеням.
 * Chroma и lightness — константы темы (`--sy-icon-*`), поэтому соседние строки
 * списка не «дерутся» яркостью.
 */

/** Восемь ступеней hue из макета дизайн-системы. */
const HUES = [32, 78, 128, 172, 218, 262, 292, 320] as const

/** FNV-1a: не крипта, а стабильный разброс строк по восьми корзинам. */
function hash(value: string): number {
  let acc = 0x811c9dc5
  for (let i = 0; i < value.length; i += 1) {
    acc ^= value.charCodeAt(i)
    acc = Math.imul(acc, 0x01000193) >>> 0
  }
  return acc
}

/** Тон плитки. Одна и та же строка всегда даёт один и тот же тон. */
export function iconHue(seed: string): number {
  const normalized = seed.trim().toLowerCase()
  if (normalized === '') return HUES[0]
  return HUES[hash(normalized) % HUES.length] ?? HUES[0]
}

/**
 * 1–2 буквы для плитки. Из двух слов берём по первой букве, из одного —
 * первую (плюс вторую заглавную, если имя вроде «GitHub»).
 */
export function iconInitials(name: string): string {
  const words = name
    .trim()
    .split(/[\s._/-]+/)
    .filter(Boolean)
  if (words.length === 0) return '?'

  if (words.length > 1) {
    return (words[0]![0]! + words[1]![0]!).toUpperCase()
  }

  const word = words[0]!
  const secondCapital = word.slice(1).match(/\p{Lu}/u)?.[0]
  return (word[0]! + (secondCapital ?? '')).toUpperCase()
}
