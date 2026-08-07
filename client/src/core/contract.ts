/**
 * IPC-контракт с Rust-ядром Syncra.
 *
 * Этот файл — ДОГОВОР с бэкенд-агентом. Любое изменение имён команд, форм
 * запросов/ответов или событий фиксируется здесь и отмечается в TASKS.md.
 *
 * Соглашения провода (предложены фронтом, финализируются с бэкендом):
 *  - Имена команд — snake_case (идиоматично для Tauri/Rust): `list_records`.
 *  - Каждая команда принимает ровно один аргумент `request`:
 *    `invoke('list_records', { request })` ⇄ `fn list_records(request: ListRecordsRequest)`.
 *  - Поля запросов/ответов — snake_case, как в §4 спека и как сериализует serde.
 *  - Даты — строки ISO-8601 в UTC.
 *
 * ЗАКОН №1: секреты (`password`, `notes`, `totp_secret`) пересекают эту границу
 * ТОЛЬКО в ответе `get_secret` — разово, по явному действию пользователя.
 * Ни одна другая команда не возвращает расшифрованный секрет, и ни один тип
 * метаданных ниже не содержит секретных полей.
 *
 * Единственное исключение — `generate_passwords` (F6): он отдаёт свежие
 * пароли-кандидаты. Это ещё ничей секрет — он не лежит в хранилище и исчезнет,
 * если пользователь не выберет ни одного варианта, — но обращаться с ним нужно
 * ровно так же: не в Pinia, не в localStorage, не в логи; живёт в области
 * видимости компонента, пока пользователь выбирает.
 */

export type RecordId = string
export type VaultId = string

/** ISO-8601 UTC, напр. `2026-08-05T21:14:03.000Z`. */
export type IsoDateTime = string

// ---------------------------------------------------------------------------
// Модель данных (§4.1 спека)
// ---------------------------------------------------------------------------

/**
 * Метаданные записи — единственное, что разрешено держать в сторах, списках,
 * поиске и любом персистентном состоянии UI. Секретных полей здесь нет
 * намеренно: их отсутствие в типе — машинно-проверяемая часть Закона №1.
 */
export interface RecordMeta {
  record_id: RecordId
  vault_id: VaultId
  /** Отображаемое имя. НЕ уникально и НЕ используется для матчинга (§4.1). */
  service_name: string
  /** Домены/URL для матчинга автозаполнения; основа группировки (§4.4). */
  urls: string[]
  login: string
  /** Метка аккаунта («личный», «рабочий») — различает аккаунты одного сервиса. */
  account_label: string | null
  /**
   * Заполнено ли поле `notes`. Это МЕТАДАННЫЕ, а не секрет: флаг говорит, что
   * заметка есть, но ничего не говорит о её содержимом. Без него карточка не
   * может отличить «заметок нет» от «заметка есть, но скрыта», не запросив
   * секрет, — то есть открывала бы секрет ради рисования рамки.
   */
  has_notes: boolean
  /**
   * Подключён ли `totp_secret`. Ровно та же логика, что у `has_notes`, плюс
   * предупреждение при удалении («вместе с записью пропадёт код подтверждения»)
   * — его надо показать ДО того, как запись удалена, и без чтения секрета.
   */
  has_totp: boolean
  /** Счётчик версий; инкрементируется ядром при каждом изменении (§5.2). */
  version: number
  created_at: IsoDateTime
  updated_at: IsoDateTime
  /** Отдельно от `updated_at` — для гигиены «паролю пора на ротацию». */
  password_updated_at: IsoDateTime
  /** Tombstone (§5.4). `null` у живых записей. */
  deleted_at: IsoDateTime | null
}

/**
 * Расшифрованные секреты записи. Приходят ТОЛЬКО из `get_secret`, разово.
 * Не класть в Pinia, localStorage, логи, снапшоты состояния и замыкания
 * дольше показа. Использовать и отпускать.
 */
export interface RecordSecrets {
  password: string
  notes: string | null
  /** Поле есть в схеме с MVP 1; генерация TOTP-кодов — фаза 2 (§4.1). */
  totp_secret: string | null
}

/** Имя секретного поля — для точечных reveal/копирования в UI. */
export type SecretField = keyof RecordSecrets

/** Данные для создания записи: метаданные + секреты на ввод. */
export interface RecordDraft {
  /** Если не указан — ядро кладёт запись в vault по умолчанию. */
  vault_id?: VaultId
  service_name: string
  urls: string[]
  login: string
  account_label?: string | null
  password: string
  notes?: string | null
  totp_secret?: string | null
}

/**
 * Патч записи. Отсутствующее поле = «не трогать».
 * Секретные поля тоже патчатся здесь: это ввод пользователя, идущий В ядро,
 * а не расшифрованное значение, идущее ИЗ ядра.
 */
export interface RecordPatch {
  vault_id?: VaultId
  service_name?: string
  urls?: string[]
  login?: string
  account_label?: string | null
  password?: string
  notes?: string | null
  totp_secret?: string | null
}

// ---------------------------------------------------------------------------
// Ошибки
// ---------------------------------------------------------------------------

export const CORE_ERROR_CODES = [
  /** Хранилище заблокировано: нужен unlock. */
  'LOCKED',
  /** Неверный мастер-пароль. */
  'INVALID_MASTER_PASSWORD',
  /** Хранилище ещё не инициализировано. */
  'NOT_INITIALIZED',
  /** `init_vault` по уже созданному хранилищу (F3). */
  'ALREADY_INITIALIZED',
  /** Запись/сущность не найдена (или является tombstone). */
  'NOT_FOUND',
  /** Запрос не прошёл валидацию ядра. */
  'VALIDATION',
  /** Непредвиденная ошибка ядра. */
  'INTERNAL',
] as const

export type CoreErrorCode = (typeof CORE_ERROR_CODES)[number]

/**
 * Форма ошибки, которую ядро возвращает через `Err(...)`.
 * `message` предназначен для показа пользователю и НИКОГДА не содержит
 * секретов, ключей, путей к хранилищу или мастер-пароля.
 */
export interface CoreErrorPayload {
  code: CoreErrorCode
  message: string
}

// ---------------------------------------------------------------------------
// Команды
// ---------------------------------------------------------------------------

/**
 * Состояние хранилища (F3). Запрашивается ДО всего остального: по нему UI
 * решает, показать онбординг, экран входа или сам продукт.
 * Команда работает и на заблокированном, и на неинициализированном хранилище.
 */
export interface VaultStatus {
  /** Хранилище создано (мастер-пароль задан, ключи сгенерированы ядром). */
  initialized: boolean
  unlocked: boolean
  /** `null`, пока хранилище заблокировано. */
  unlocked_at: IsoDateTime | null
}

export type GetVaultStatusRequest = Record<string, never>
export type GetVaultStatusResponse = VaultStatus

/**
 * Первичная инициализация (F3, §3.9).
 *
 * UI шлёт ТОЛЬКО введённый пользователем мастер-пароль. Генерация ключей,
 * KDF и создание файла хранилища — целиком в ядре: на фронте крипты нет.
 * Успешная инициализация сразу оставляет хранилище разблокированным —
 * заставлять пользователя вводить пароль второй раз подряд бессмысленно.
 */
export interface InitVaultRequest {
  master_password: string
}

export interface InitVaultResponse {
  initialized_at: IsoDateTime
  unlocked_at: IsoDateTime
}

/**
 * Минимальная длина мастер-пароля. Политику задаёт ядро — здесь она продублирована,
 * чтобы UI мог подсветить проблему до отправки команды. Ядро всё равно проверяет
 * повторно и возвращает `VALIDATION`.
 */
export const MASTER_PASSWORD_MIN_LENGTH = 8

export interface UnlockRequest {
  master_password: string
}

export interface UnlockResponse {
  unlocked_at: IsoDateTime
}

export interface ListRecordsRequest {
  /** Ограничить одной секцией. По умолчанию — все секции. */
  vault_id?: VaultId
  /** Включить tombstones. По умолчанию `false`. */
  include_deleted?: boolean
}

export type ListRecordsResponse = RecordMeta[]

export interface GetSecretRequest {
  record_id: RecordId
}

export type GetSecretResponse = RecordSecrets

export interface CreateRecordRequest {
  draft: RecordDraft
}

export interface UpdateRecordRequest {
  record_id: RecordId
  patch: RecordPatch
}

export interface DeleteRecordRequest {
  record_id: RecordId
}

// ---------------------------------------------------------------------------
// Генератор паролей (F6, §6.1)
// ---------------------------------------------------------------------------

/**
 * Как собирается пароль: из отдельных символов или из слов (парольная фраза).
 * Фраза — не «режим для слабаков»: четыре случайных слова дают больше энтропии,
 * чем короткий «Qw!7z», и их можно продиктовать вслух.
 */
export type GeneratorMode = 'chars' | 'words'

/** Чем склеиваются слова фразы. Пробел — тоже допустимый разделитель. */
export type WordSeparator = '-' | '.' | ' '

/**
 * Сохранённый профиль генерации (§6.1).
 *
 * UX-принцип спека: пользователь настраивает правила ОДИН РАЗ, а не при каждом
 * использовании. Профиль — это настройки, а не секрет: в нём нет ни одного
 * пароля, поэтому его можно держать в сторе и показывать на экране настроек.
 *
 * Поля обоих режимов лежат рядом намеренно: переключение `chars` ⇄ `words` не
 * должно терять то, что пользователь уже настроил в другом режиме.
 */
export interface GeneratorProfile {
  mode: GeneratorMode
  /** Длина пароля в символах. Режим `chars`. */
  length: number
  /** Добавлять цифры. Режим `chars`. */
  digits: boolean
  /** Добавлять спецсимволы. Режим `chars`. */
  symbols: boolean
  /**
   * Исключать похожие символы (`0`/`O`, `1`/`l`/`I`). Режим `chars`.
   * Буквы в алфавите есть всегда — профиля с пустым алфавитом не существует.
   */
  avoid_ambiguous: boolean
  /** Сколько слов в фразе. Режим `words`. */
  words: number
  separator: WordSeparator
  /** Дописывать число в конец фразы — некоторые сайты требуют цифру. */
  append_number: boolean
}

/**
 * Границы, внутри которых ядро принимает профиль. Политику задаёт ЯДРО —
 * здесь она продублирована, чтобы ползунки имели min/max и UI мог подсветить
 * проблему до отправки. Ядро проверяет повторно и отвечает `VALIDATION`.
 */
export const GENERATOR_LIMITS = {
  length: { min: 8, max: 40 },
  words: { min: 3, max: 7 },
  /** Сколько вариантов можно попросить за раз. */
  count: { min: 1, max: 10 },
} as const

/** Сколько вариантов показывает форма записи по умолчанию (из макета §3.4). */
export const GENERATOR_DEFAULT_COUNT = 5

export type GetGeneratorProfileRequest = Record<string, never>
export type GetGeneratorProfileResponse = GeneratorProfile

export interface SaveGeneratorProfileRequest {
  profile: GeneratorProfile
}

/** Ядро возвращает профиль после нормализации — его и показываем. */
export type SaveGeneratorProfileResponse = GeneratorProfile

export interface GeneratePasswordsRequest {
  /** Сколько вариантов на выбор (§6.1: «пользователь указывает n»). */
  count: number
  /**
   * Разовые правила — для предпросмотра на экране настроек, пока профиль ещё
   * не сохранён. Не указан — ядро берёт сохранённый профиль.
   */
  profile?: GeneratorProfile
}

/**
 * ВНИМАНИЕ: единственный ответ кроме `get_secret`, содержащий пароли открытым
 * текстом. Правила обращения — в шапке файла.
 */
export interface GeneratePasswordsResponse {
  passwords: string[]
  /**
   * Оценка стойкости, которую считает ЯДРО. Фронт её не выводит сам: только
   * ядро знает настоящий алфавит и размер словаря, а считать «на глаз» —
   * значит врать пользователю о стойкости его пароля.
   */
  entropy_bits: number
}

/**
 * Карта команд: имя метода клиента → форма запроса/ответа.
 * Служит единым источником истины для `ipc.ts` и мок-ядра.
 */
export interface CommandMap {
  getVaultStatus: { request: GetVaultStatusRequest; response: GetVaultStatusResponse }
  initVault: { request: InitVaultRequest; response: InitVaultResponse }
  unlock: { request: UnlockRequest; response: UnlockResponse }
  lock: { request: Record<string, never>; response: null }
  listRecords: { request: ListRecordsRequest; response: ListRecordsResponse }
  getSecret: { request: GetSecretRequest; response: GetSecretResponse }
  createRecord: { request: CreateRecordRequest; response: RecordMeta }
  updateRecord: { request: UpdateRecordRequest; response: RecordMeta }
  /** Возвращает tombstone-метаданные удалённой записи (§5.4). */
  deleteRecord: { request: DeleteRecordRequest; response: RecordMeta }
  getGeneratorProfile: {
    request: GetGeneratorProfileRequest
    response: GetGeneratorProfileResponse
  }
  saveGeneratorProfile: {
    request: SaveGeneratorProfileRequest
    response: SaveGeneratorProfileResponse
  }
  generatePasswords: { request: GeneratePasswordsRequest; response: GeneratePasswordsResponse }
}

export type CommandName = keyof CommandMap

/** Имена команд на проводе (Tauri `invoke`). */
export const COMMAND_NAMES: Record<CommandName, string> = {
  getVaultStatus: 'get_vault_status',
  initVault: 'init_vault',
  unlock: 'unlock',
  lock: 'lock',
  listRecords: 'list_records',
  getSecret: 'get_secret',
  createRecord: 'create_record',
  updateRecord: 'update_record',
  deleteRecord: 'delete_record',
  getGeneratorProfile: 'get_generator_profile',
  saveGeneratorProfile: 'save_generator_profile',
  generatePasswords: 'generate_passwords',
}

// ---------------------------------------------------------------------------
// События
// ---------------------------------------------------------------------------

export interface UnlockedEvent {
  unlocked_at: IsoDateTime
}

export interface LockedEvent {
  locked_at: IsoDateTime
  /** Почему заблокировались: явное действие, таймаут бездействия, сон системы. */
  reason: 'manual' | 'timeout' | 'system'
}

/**
 * Карта событий ядра. Расширяется по мере задач
 * (`sync_status`, `peer_found` — F10; `conflict_raised` — F11).
 */
export interface EventMap {
  unlocked: UnlockedEvent
  locked: LockedEvent
}

export type EventName = keyof EventMap

/** Имена событий на проводе (Tauri `listen`). */
export const EVENT_NAMES: Record<EventName, string> = {
  unlocked: 'unlocked',
  locked: 'locked',
}
