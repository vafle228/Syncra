import type { Device, DeviceKind, QrMatrix } from '../contract'
import { PAIRING_MANUAL_CODE_LENGTH } from '../contract'
import { CoreError } from '../errors'

/**
 * Сопряжение устройств в фейк-ядре (F8, §2.2).
 *
 * Что здесь ЧЕСТНО, а что подделка, — важно не перепутать:
 *
 *  - Настоящий QR-код тут не кодируется. `buildQrMatrix` рисует правдоподобную
 *    матрицу: поисковые узоры по углам, синхрополосы, «данные» из хэша
 *    пейлоада. Прочитать её сканером нельзя, и это не недоделка — кодированием
 *    занимается Rust-ядро, у которого есть и настоящий пейлоад, и библиотека
 *    под него. UI одинаково рисует и ту матрицу, и эту: он получает картинку.
 *  - Ключей здесь нет вообще. В настоящем ядре в пейлоаде лежит публичный ключ
 *    и одноразовый ключ сеанса; фейк-ядро обходится случайной строкой —
 *    крипты во фронте нет и в моке её быть не должно.
 *  - Случайность кода — настоящая, у ОС (`crypto.getRandomValues`). Код
 *    сопряжения даёт право забрать копию хранилища: предсказуемым он не должен
 *    быть даже в деве.
 */

/**
 * Сколько живёт код. В макете (§3.6) на экране обратный отсчёт «код живёт
 * 02:41» — то есть порядка трёх минут: столько нужно, чтобы дойти до второго
 * устройства, и мало, чтобы забытый на экране код кто-то успел использовать.
 */
export const PAIRING_CODE_TTL_MS = 3 * 60 * 1000

/**
 * Алфавит кода для ручного ввода: ни `0`/`O`, ни `1`/`I`/`L`. Код диктуют
 * вслух и набирают руками — похожие начертания здесь стоят пользователю
 * лишней попытки.
 *
 * Ровно 32 символа, поэтому `байт % 32` равновероятен и отбраковка не нужна.
 */
const CODE_ALPHABET = '23456789ABCDEFGHJKLMNPQRSTUVWXYZ'

/** Префикс полного пейлоада — по нему ядро отличает QR от набранного кода. */
export const PAIRING_PAYLOAD_PREFIX = 'syncra-pair:'

/** Длина случайного «ключа сеанса» в пейлоаде мока, шестнадцатеричных цифр. */
const PAYLOAD_KEY_HEX = 32

function randomBytes(count: number): Uint8Array {
  const buffer = new Uint8Array(count)
  crypto.getRandomValues(buffer)
  return buffer
}

/** Код для ручного ввода: `4TQ9MB` в каноническом виде, без разделителей. */
export function createManualCode(): string {
  let code = ''
  for (const byte of randomBytes(PAIRING_MANUAL_CODE_LENGTH)) {
    code += CODE_ALPHABET[byte % CODE_ALPHABET.length]
  }
  return code
}

/** Полный пейлоад, который уезжает в QR. В настоящем ядре здесь ключи. */
export function createPairingPayload(manualCode: string): string {
  let key = ''
  for (const byte of randomBytes(PAYLOAD_KEY_HEX / 2)) {
    key += byte.toString(16).padStart(2, '0')
  }
  return `${PAIRING_PAYLOAD_PREFIX}${manualCode.toLowerCase()}.${key}`
}

/**
 * Приводит прочитанное к каноническому коду.
 *
 * Принимает и полный пейлоад из QR, и код, набранный руками, — в том числе с
 * пробелами, точками-разделителями и в нижнем регистре: человек, переписывающий
 * код с чужого экрана, не обязан попадать в наш формат.
 *
 * `null` — это не код Syncra.
 */
export function normalizePairingInput(raw: string): string | null {
  const trimmed = raw.trim()

  if (trimmed.toLowerCase().startsWith(PAIRING_PAYLOAD_PREFIX)) {
    const body = trimmed.slice(PAIRING_PAYLOAD_PREFIX.length)
    const match = /^([0-9a-z]{6})\.[0-9a-f]{32}$/i.exec(body)
    return match ? normalizeCode(match[1] as string) : null
  }

  return normalizeCode(trimmed)
}

/** Только сам код: разделители выбрасываем, регистр поднимаем. */
function normalizeCode(raw: string): string | null {
  const code = raw.replace(/[\s.·-]/g, '').toUpperCase()
  if (code.length !== PAIRING_MANUAL_CODE_LENGTH) return null
  return [...code].every((char) => CODE_ALPHABET.includes(char)) ? code : null
}

// ---------------------------------------------------------------------------
// Матрица QR (подделка, см. шапку файла)
// ---------------------------------------------------------------------------

/** Сторона матрицы. Соответствует QR версии 2 — размер правдоподобный. */
export const FAKE_QR_SIZE = 25

/** FNV-1a: нужен детерминизм, а не стойкость — это не хэш для крипты. */
function hash32(input: string): number {
  let value = 0x811c9dc5
  for (let i = 0; i < input.length; i += 1) {
    value ^= input.charCodeAt(i)
    value = Math.imul(value, 0x01000193) >>> 0
  }
  return value
}

/** xorshift32 — детерминированный поток «данных» матрицы от одного зерна. */
function bitStream(seed: number): () => boolean {
  let state = seed === 0 ? 0x9e3779b9 : seed
  return () => {
    state ^= state << 13
    state ^= state >>> 17
    state ^= state << 5
    state >>>= 0
    return (state & 1) === 1
  }
}

/** Поисковый узор 7×7: рамка, светлое кольцо, тёмный центр 3×3. */
function isFinderModule(x: number, y: number): boolean {
  const ring = Math.max(Math.abs(x - 3), Math.abs(y - 3))
  return ring === 3 || ring <= 1
}

/**
 * Правдоподобная матрица кода. Детерминирована по пейлоаду: один и тот же
 * код всегда рисуется одинаково, поэтому его можно проверить тестом.
 */
export function buildQrMatrix(payload: string, size: number = FAKE_QR_SIZE): QrMatrix {
  const nextBit = bitStream(hash32(payload))
  const rows: string[] = []

  /** Углы с поисковыми узорами: левый верхний, правый верхний, левый нижний. */
  const finders = [
    { x: 0, y: 0 },
    { x: size - 7, y: 0 },
    { x: 0, y: size - 7 },
  ]

  for (let y = 0; y < size; y += 1) {
    let row = ''
    for (let x = 0; x < size; x += 1) {
      const finder = finders.find(
        (corner) => x >= corner.x && x < corner.x + 7 && y >= corner.y && y < corner.y + 7,
      )
      if (finder) {
        row += isFinderModule(x - finder.x, y - finder.y) ? '1' : '0'
        continue
      }

      // Светлая рамка вокруг поисковых узоров.
      const nearFinder = finders.some(
        (corner) =>
          x >= corner.x - 1 && x <= corner.x + 7 && y >= corner.y - 1 && y <= corner.y + 7,
      )
      if (nearFinder) {
        row += '0'
        continue
      }

      // Синхрополосы: шестая строка и шестой столбец чередуются.
      if (x === 6 || y === 6) {
        row += (x + y) % 2 === 0 ? '1' : '0'
        continue
      }

      row += nextBit() ? '1' : '0'
    }
    rows.push(row)
  }

  return { size, rows }
}

// ---------------------------------------------------------------------------
// Отпечаток сеанса и второе устройство
// ---------------------------------------------------------------------------

/**
 * Слова отпечатка. В настоящем ядре на их месте выверенный список (вроде
 * PGP word list), в котором слова не путаются на слух: их сверяют голосом
 * через комнату. Здесь короткий — как и словарь генератора, он честно
 * маленький, а не притворяется большим.
 */
const FINGERPRINT_WORDS = [
  'якорь',
  'бархат',
  'вереск',
  'гранит',
  'дельта',
  'ельник',
  'жёлудь',
  'зенит',
  'изумруд',
  'кварц',
  'ландыш',
  'маяк',
  'невод',
  'оникс',
  'парус',
  'ротонда',
]

/** Сколько слов сверяет человек. Четыре — как в макете (§3.6). */
export const FINGERPRINT_WORD_COUNT = 4

/**
 * Отпечаток сеанса. Считается из кода, поэтому на обоих устройствах выходит
 * одинаковым — ровно то свойство, ради которого его и сверяют (§2.2).
 */
export function fingerprintWords(code: string): string[] {
  const nextBit = bitStream(hash32(`fingerprint:${code}`))
  const words: string[] = []

  for (let i = 0; i < FINGERPRINT_WORD_COUNT; i += 1) {
    let index = 0
    for (let bit = 0; bit < 4; bit += 1) index = (index << 1) | (nextBit() ? 1 : 0)
    words.push(FINGERPRINT_WORDS[index % FINGERPRINT_WORDS.length] as string)
  }

  return words
}

/**
 * Кто на том конце. Настоящее ядро узнаёт имя устройства при обмене ключами;
 * фейк-ядро выдумывает его детерминированно по коду — иначе экран успеха
 * пришлось бы подписывать «устройство».
 */
const PEER_DEVICES: { name: string; kind: DeviceKind }[] = [
  { name: 'iPhone 14', kind: 'mobile' },
  { name: 'Pixel 8', kind: 'mobile' },
  { name: 'iPad', kind: 'mobile' },
  { name: 'MacBook Air', kind: 'desktop' },
  { name: 'ThinkPad X1', kind: 'desktop' },
]

export function peerDevice(code: string): { name: string; kind: DeviceKind } {
  return PEER_DEVICES[hash32(`peer:${code}`) % PEER_DEVICES.length] as {
    name: string
    kind: DeviceKind
  }
}

/**
 * Как называется устройство, на котором открыт UI. Нужно не только списку
 * устройств: в конфликте версий (F11) местная сторона подписывается именно им —
 * «какая версия чья» без имени не читается.
 */
export const THIS_DEVICE_NAME = 'Этот компьютер'

/**
 * Устройство, на котором открыт UI. В настоящем ядре его имя берётся у ОС;
 * фейк-ядру достаточно, чтобы список доверенных устройств не был пустым.
 */
export function createThisDevice(pairedAt: string): Device {
  return {
    device_id: 'device-this',
    name: THIS_DEVICE_NAME,
    kind: 'desktop',
    is_this_device: true,
    paired_at: pairedAt,
    fingerprint_words: fingerprintWords('device-this'),
    last_seen_at: pairedAt,
    revoked_at: null,
  }
}

/** Единая формулировка отказа: код не похож на код Syncra. */
export function unreadableCode(): CoreError {
  return new CoreError(
    'VALIDATION',
    'Это не похоже на код Syncra. Проверьте шесть символов со второго устройства.',
  )
}
