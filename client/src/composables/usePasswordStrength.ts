import { computed, type Ref } from 'vue'

import { MASTER_PASSWORD_MIN_LENGTH } from '@/core/contract'

/**
 * Подсказка о силе мастер-пароля для экрана создания хранилища (F3, §3.9).
 *
 * Это ЧИСТО ВИЗУАЛЬНАЯ эвристика: четыре полоски и фраза. Никакой криптографии,
 * энтропии «по-настоящему» и обращений в сеть — оценку не с чем сверять, да и
 * не нужно: решение о допустимости пароля принимает ядро (`VALIDATION`).
 * Пароль сюда приходит по ссылке на ref формы и никуда дальше не уходит.
 */

export type StrengthLevel = 0 | 1 | 2 | 3 | 4

const LABELS: Record<StrengthLevel, string> = {
  0: `Слишком короткий — нужно хотя бы ${MASTER_PASSWORD_MIN_LENGTH} символов`,
  1: 'Слабый — его подберут перебором',
  2: 'Средний — сойдёт, но можно лучше',
  3: 'Хороший',
  4: 'Отличный — такую фразу не подберут',
}

export function scorePassword(value: string): StrengthLevel {
  if (value.length < MASTER_PASSWORD_MIN_LENGTH) return 0

  let score = 1
  // Длина решает больше, чем набор символов: фраза из слов надёжнее «P@ss1».
  if (value.length >= 12) score += 1
  if (value.length >= 20) score += 1
  // Разнообразие — небольшой бонус, а не главный критерий.
  const classes = [/\p{Ll}/u, /\p{Lu}/u, /\d/u, /[^\p{L}\d]/u].filter((re) => re.test(value)).length
  if (classes >= 3) score += 1

  return Math.min(score, 4) as StrengthLevel
}

export function usePasswordStrength(password: Ref<string>) {
  const level = computed(() => scorePassword(password.value))

  return {
    level,
    label: computed(() => LABELS[level.value]),
    /** Достаточно ли для отправки в ядро (ядро проверит ещё раз). */
    acceptable: computed(() => password.value.length >= MASTER_PASSWORD_MIN_LENGTH),
  }
}
