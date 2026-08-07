import type { GeneratePasswordsResponse, GeneratorProfile } from '../contract'
import { GENERATOR_LIMITS } from '../contract'
import { CoreError } from '../errors'

/**
 * Генератор паролей фейк-ядра (F6, §6.1).
 *
 * Почему этот файл лежит в `src/core/mock/`, а не в `composables`: генерация —
 * работа ЯДРА. В настоящем продукте её делает Rust, и UI получает только готовые
 * строки через `generate_passwords`. Здесь фейк-ядро исполняет ту же роль, и
 * граница проходит там же — компоненты про этот код ничего не знают.
 *
 * Случайность берётся у ОС (`crypto.getRandomValues`), а не у `Math.random`.
 * Это не «крипта во фронте», запрещённая CLAUDE.md (там про шифрование, KDF и
 * подписи): это отказ выдавать за пароль предсказуемую последовательность.
 * Мок-паролем никто не должен защищать настоящий аккаунт — но если это всё-таки
 * случится в деве, пусть пароль хотя бы будет настоящим случайным.
 */

/** Буквы без похожих начертаний: ни `l`, ни `I`, ни `O`. */
const LETTERS_CLEAR = 'abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ'
/** Те самые спорные символы — добавляются, когда фильтр выключен. */
const LETTERS_AMBIGUOUS = 'iloIO'
const DIGITS_CLEAR = '23456789'
const DIGITS_AMBIGUOUS = '01'
const SYMBOLS = '!@#$%^&*-_=+?'

/**
 * Словарь фейк-ядра. В настоящем ядре на его месте полноценный список (тысячи
 * слов) — от размера словаря напрямую зависит стойкость фразы. Здесь список
 * короткий, и оценка энтропии ниже считается по его РЕАЛЬНОЙ длине: врать про
 * стойкость нельзя даже в моке.
 */
const WORD_BANK = [
  'кедр',
  'фарфор',
  'мостик',
  'янтарь',
  'ливень',
  'сокол',
  'пристань',
  'гравий',
  'вереск',
  'компас',
  'торф',
  'ясень',
  'парус',
  'изразец',
  'полынь',
  'причал',
  'сумерки',
  'береста',
  'колодец',
  'наледь',
]

const SEPARATORS: GeneratorProfile['separator'][] = ['-', '.', ' ']

/** Профиль, с которым ядро отдаёт свежесозданное хранилище. */
export const DEFAULT_GENERATOR_PROFILE: GeneratorProfile = {
  mode: 'chars',
  length: 20,
  digits: true,
  symbols: true,
  avoid_ambiguous: true,
  words: 4,
  separator: '-',
  append_number: false,
}

export function cloneProfile(profile: GeneratorProfile): GeneratorProfile {
  return { ...profile }
}

// ---------------------------------------------------------------------------
// Случайность
// ---------------------------------------------------------------------------

/**
 * Равномерное целое из `[0, maxExclusive)`.
 *
 * Через отбраковку хвоста, а не через `% maxExclusive`: остаток от деления
 * 32-битного значения даёт перекос в сторону младших вариантов, и алфавит
 * перестаёт быть равновероятным. Для генератора паролей это прямая потеря
 * стойкости — той самой, про которую мы рядом пишем «≈ N бит».
 */
function randomInt(maxExclusive: number): number {
  const limit = Math.floor(0x1_0000_0000 / maxExclusive) * maxExclusive
  const buffer = new Uint32Array(1)
  let value = 0
  do {
    crypto.getRandomValues(buffer)
    value = buffer[0] as number
  } while (value >= limit)
  return value % maxExclusive
}

function pick<T>(items: readonly T[]): T {
  return items[randomInt(items.length)] as T
}

// ---------------------------------------------------------------------------
// Валидация профиля
// ---------------------------------------------------------------------------

function inRange(value: number, range: { min: number; max: number }): boolean {
  return Number.isInteger(value) && value >= range.min && value <= range.max
}

/**
 * Проверяет профиль так же придирчиво, как настоящее ядро: UI мог отправить
 * что угодно, и «доверять фронту» — не тот подход, который стоит репетировать
 * даже в фейк-ядре.
 */
export function validateProfile(profile: GeneratorProfile): GeneratorProfile {
  if (profile.mode !== 'chars' && profile.mode !== 'words') {
    throw new CoreError('VALIDATION', 'Неизвестный режим генерации.')
  }
  if (!inRange(profile.length, GENERATOR_LIMITS.length)) {
    throw new CoreError(
      'VALIDATION',
      `Длина пароля — от ${GENERATOR_LIMITS.length.min} до ${GENERATOR_LIMITS.length.max} символов.`,
    )
  }
  if (!inRange(profile.words, GENERATOR_LIMITS.words)) {
    throw new CoreError(
      'VALIDATION',
      `Слов во фразе — от ${GENERATOR_LIMITS.words.min} до ${GENERATOR_LIMITS.words.max}.`,
    )
  }
  if (!SEPARATORS.includes(profile.separator)) {
    throw new CoreError('VALIDATION', 'Недопустимый разделитель слов.')
  }

  return {
    mode: profile.mode,
    length: profile.length,
    digits: profile.digits === true,
    symbols: profile.symbols === true,
    avoid_ambiguous: profile.avoid_ambiguous === true,
    words: profile.words,
    separator: profile.separator,
    append_number: profile.append_number === true,
  }
}

// ---------------------------------------------------------------------------
// Генерация
// ---------------------------------------------------------------------------

/** Алфавит режима `chars`. Буквы есть всегда — пустого алфавита не бывает. */
function alphabet(profile: GeneratorProfile): string {
  let result = LETTERS_CLEAR
  if (!profile.avoid_ambiguous) result += LETTERS_AMBIGUOUS
  if (profile.digits) result += DIGITS_CLEAR + (profile.avoid_ambiguous ? '' : DIGITS_AMBIGUOUS)
  if (profile.symbols) result += SYMBOLS
  return result
}

/** Диапазон приписываемого числа — двузначное, как в макете. */
const APPENDED_NUMBER_RANGE = 90
const APPENDED_NUMBER_MIN = 10

function generateOne(profile: GeneratorProfile): string {
  if (profile.mode === 'words') {
    const separator = profile.separator
    const parts: string[] = []
    for (let i = 0; i < profile.words; i += 1) parts.push(pick(WORD_BANK))
    if (profile.append_number) {
      parts.push(String(randomInt(APPENDED_NUMBER_RANGE) + APPENDED_NUMBER_MIN))
    }
    return parts.join(separator)
  }

  const chars = alphabet(profile)
  let password = ''
  for (let i = 0; i < profile.length; i += 1) password += chars[randomInt(chars.length)]
  return password
}

/**
 * Оценка стойкости в битах. Считается по тому же алфавиту/словарю, которым
 * пароль на самом деле собран, — иначе цифра рядом с паролем была бы украшением.
 */
export function entropyBits(profile: GeneratorProfile): number {
  if (profile.mode === 'words') {
    const fromWords = profile.words * Math.log2(WORD_BANK.length)
    const fromNumber = profile.append_number ? Math.log2(APPENDED_NUMBER_RANGE) : 0
    return Math.round(fromWords + fromNumber)
  }
  return Math.round(profile.length * Math.log2(alphabet(profile).length))
}

/** `count` вариантов по готовому (уже проверенному) профилю. */
export function generatePasswords(
  profile: GeneratorProfile,
  count: number,
): GeneratePasswordsResponse {
  if (!inRange(count, GENERATOR_LIMITS.count)) {
    throw new CoreError(
      'VALIDATION',
      `Вариантов можно попросить от ${GENERATOR_LIMITS.count.min} до ${GENERATOR_LIMITS.count.max}.`,
    )
  }

  const passwords: string[] = []
  for (let i = 0; i < count; i += 1) passwords.push(generateOne(profile))

  return { passwords, entropy_bits: entropyBits(profile) }
}
