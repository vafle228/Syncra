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

/**
 * Карта команд: имя метода клиента → форма запроса/ответа.
 * Служит единым источником истины для `ipc.ts` и мок-ядра.
 */
export interface CommandMap {
  unlock: { request: UnlockRequest; response: UnlockResponse }
  lock: { request: Record<string, never>; response: null }
  listRecords: { request: ListRecordsRequest; response: ListRecordsResponse }
  getSecret: { request: GetSecretRequest; response: GetSecretResponse }
  createRecord: { request: CreateRecordRequest; response: RecordMeta }
  updateRecord: { request: UpdateRecordRequest; response: RecordMeta }
  /** Возвращает tombstone-метаданные удалённой записи (§5.4). */
  deleteRecord: { request: DeleteRecordRequest; response: RecordMeta }
}

export type CommandName = keyof CommandMap

/** Имена команд на проводе (Tauri `invoke`). */
export const COMMAND_NAMES: Record<CommandName, string> = {
  unlock: 'unlock',
  lock: 'lock',
  listRecords: 'list_records',
  getSecret: 'get_secret',
  createRecord: 'create_record',
  updateRecord: 'update_record',
  deleteRecord: 'delete_record',
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
