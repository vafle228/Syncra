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
 * ТОЛЬКО разово и только по явному действию пользователя. Ни один тип
 * метаданных ниже не содержит секретных полей.
 *
 * Команд, отдающих открытый текст, ровно три, и все они — по нажатию:
 *  - `get_secret` (F5) — reveal или копирование одного поля записи;
 *  - `get_conflict_secret` (F11) — то же поле в двух версиях сразу, чтобы
 *    человек мог сравнить их при разрешении конфликта (§5.5);
 *  - `generate_passwords` (F6) — свежие пароли-кандидаты. Это ещё ничей
 *    секрет: он не лежит в хранилище и исчезнет, если ни один вариант не
 *    выбран, — но обращаться с ним нужно так же.
 *
 * Правило обращения общее для всех трёх: не в Pinia, не в localStorage, не в
 * логи; значение живёт в области видимости компонента, пока оно на экране.
 *
 * Их по-прежнему три и после F12: экспорт (CSV и бэкап) файл СОБИРАЕТ И ПИШЕТ
 * сам, а UI получает только путь к нему — см. `ExportFile`. Импорт устроен
 * зеркально: разобранные чужие пароли остаются в ядре до подтверждения, через
 * границу идут метаданные предпросмотра.
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
  /** Код сопряжения устарел: срок жизни вышел (F8, §2.2). */
  'PAIRING_EXPIRED',
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
// Секции / vaults (F7, §4.2)
// ---------------------------------------------------------------------------

/**
 * Цвет метки секции. На проводе — ИМЯ ступени палитры, а не CSS-цвет: набор
 * цветов задаёт дизайн-система, и хранить в ядре строку `oklch(...)` значило бы
 * зашить оформление в данные, которые синхронизируются между устройствами и
 * переживут смену темы.
 */
export type VaultColor = 'indigo' | 'amber' | 'magenta' | 'mint' | 'coral'

/** Порядок — как в палитре макета (§ «Секции»). */
export const VAULT_COLORS = ['indigo', 'amber', 'magenta', 'mint', 'coral'] as const

/**
 * Секция (§4.2).
 *
 * Секция — это ПАПКА, а не отдельное хранилище: ключ у всех секций один, и при
 * автозаполнении подходящая запись предлагается независимо от того, в какой
 * секции лежит. Единственное, что секция правда решает, — уезжает ли её
 * содержимое на другие устройства (`sync`).
 *
 * Секретов здесь нет: имя и цвет — метаданные, как и всё остальное в списках.
 */
export interface Vault {
  vault_id: VaultId
  name: string
  color: VaultColor
  /**
   * Синхронизировать секцию между устройствами (§4.2). `false` — секция
   * остаётся локальной: её записи не уезжают с этого устройства и не появятся
   * на других, даже сопряжённых.
   */
  sync: boolean
  /**
   * Куда ложится запись, созданная без явной секции. Помечена ровно одна
   * секция — иначе `create_record` без `vault_id` было бы некуда положить.
   */
  is_default: boolean
  created_at: IsoDateTime
}

/**
 * Максимальная длина имени секции. Политику задаёт ЯДРО — здесь продублирована
 * ради `maxlength` в поле ввода; ядро проверяет повторно (`VALIDATION`).
 */
export const VAULT_NAME_MAX_LENGTH = 40

export type ListVaultsRequest = Record<string, never>
export type ListVaultsResponse = Vault[]

export interface CreateVaultRequest {
  name: string
  color: VaultColor
}

/** Что можно поменять у секции. Отсутствующее поле = «не трогать». */
export interface VaultPatch {
  name?: string
  color?: VaultColor
}

export interface UpdateVaultRequest {
  vault_id: VaultId
  patch: VaultPatch
}

/**
 * Флаг синхронизации вынесен из `update_vault` намеренно: переименование —
 * косметика, а этот тумблер решает, покидают ли записи устройство. Такие вещи
 * не стоит уметь менять «заодно», патчем вместе с цветом.
 */
export interface SetVaultSyncRequest {
  vault_id: VaultId
  sync: boolean
}

export interface SetDefaultVaultRequest {
  vault_id: VaultId
}

export interface DeleteVaultRequest {
  vault_id: VaultId
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

// ---------------------------------------------------------------------------
// Сопряжение устройств (F8, §2.2)
// ---------------------------------------------------------------------------

export type DeviceId = string
export type PairingSessionId = string

/** Тип устройства — только для иконки и подписи. В доверии не участвует (§2.1). */
export type DeviceKind = 'desktop' | 'mobile'

/**
 * Доверенное устройство (§2.2).
 *
 * Корень доверия — публичный ключ, и он остаётся В ЯДРЕ: сюда приходит только
 * то, что нужно нарисовать список. Ни ключа, ни MAC-адреса в этом типе нет —
 * MAC вообще не основание для доверия (§2.1), а показывать сетевые адреса
 * пользователю незачем.
 */
export interface Device {
  device_id: DeviceId
  name: string
  kind: DeviceKind
  /** Устройство, на котором открыт этот UI. Себя отозвать нельзя (F9). */
  is_this_device: boolean
  paired_at: IsoDateTime
  /** Когда устройство последний раз выходило на связь. `null` — ни разу. */
  last_seen_at: IsoDateTime | null
  /** Отзыв доступа (§2.3). `null` у действующих устройств. */
  revoked_at: IsoDateTime | null
}

/**
 * Готовая матрица QR-кода.
 *
 * Кодирует ядро, а не UI. Причина не в удобстве: в пейлоаде лежит одноразовый
 * ключ сеанса, и чем меньше он существует в виде JS-строки, тем лучше. UI
 * получает КАРТИНКУ кода, рисует её и ничего не знает о содержимом.
 */
export interface QrMatrix {
  /** Сторона матрицы в модулях (без полей вокруг — их добавляет UI). */
  size: number
  /** `size` строк по `size` символов: `'1'` — тёмный модуль, `'0'` — светлый. */
  rows: string[]
}

/**
 * Код, который устройство показывает второму (§2.2, §3.6 макета).
 *
 * Живёт ограниченное время: одноразовый ключ сеанса, повисший на экране
 * до утра, — это приглашение сопрячься для любого, кто мимо прошёл.
 */
export interface PairingOffer {
  session_id: PairingSessionId
  qr: QrMatrix
  /**
   * Тот же код словами-цифрами для ручного переноса, когда камеры нет.
   * Канонический вид — без разделителей; их для читаемости расставляет UI.
   */
  manual_code: string
  expires_at: IsoDateTime
}

/**
 * Что второе устройство узнало, прочитав код. Сопряжение на этом ещё НЕ
 * состоялось: ключами обменялись, но подтверждения человека нет.
 *
 * `fingerprint_words` — та самая защита от MITM (§2.2): слова считаются из
 * ключей обеих сторон и должны совпасть на двух экранах. Совпали — посередине
 * никого нет. Поэтому ядро возвращает их ДО того, как запишет устройство в
 * доверенные, а не после.
 */
export interface PairingHandshake {
  session_id: PairingSessionId
  peer_name: string
  peer_kind: DeviceKind
  fingerprint_words: string[]
  /** Докуда действителен этот сеанс: сверять отпечаток вечно нельзя. */
  expires_at: IsoDateTime
}

/** Чем закончилось сопряжение. */
export interface PairingResult {
  device: Device
  /**
   * Сколько записей уехало на новое устройство. Записи локальных секций сюда
   * НЕ входят: выключенная синхронизация означает «не уезжает никуда», в том
   * числе на только что сопряжённое устройство (§4.2).
   */
  records_transferred: number
  /** Сколько занял перенос. Считает ядро — оно его и делало. */
  duration_ms: number
}

/** Длина кода для ручного ввода. Политика ЯДРА, здесь — ради `maxlength`. */
export const PAIRING_MANUAL_CODE_LENGTH = 6

export type GetPairingPayloadRequest = Record<string, never>
export type GetPairingPayloadResponse = PairingOffer

/**
 * Отдать ядру то, что прочитали со второго устройства.
 *
 * Один строковый `payload` вместо двух полей намеренно: UI не разбирает код и
 * не знает его формата — он не отличает полный пейлоад из QR от шестизначного
 * кода, набранного руками. Разбирает ядро, оно же отвечает `VALIDATION`, если
 * это вообще не код Syncra.
 */
export interface SubmitPairedKeyRequest {
  payload: string
}

export type SubmitPairedKeyResponse = PairingHandshake

/** Человек сверил слова на двух экранах и подтвердил (§2.2). */
export interface ConfirmPairingRequest {
  session_id: PairingSessionId
}

export type ConfirmPairingResponse = PairingResult

export interface CancelPairingRequest {
  session_id: PairingSessionId
}

// ---------------------------------------------------------------------------
// Доверенные устройства и отзыв (F9, §2.3)
// ---------------------------------------------------------------------------

/**
 * Список доверенных устройств.
 *
 * Отозванные устройства из списка НЕ исчезают: отзыв — это факт истории
 * хранилища, а не удаление строки. Человек, который отозвал украденный ноутбук,
 * должен видеть, что он отозван, а не то, что его никогда не было. Отличить их
 * можно по `revoked_at`.
 */
export type ListDevicesRequest = Record<string, never>
export type ListDevicesResponse = Device[]

/**
 * Отозвать доступ у устройства (§2.3).
 *
 * Что отзыв делает и чего НЕ делает — важно не перепутать, и UI обязан говорить
 * об этом прямо: отозванное устройство перестаёт получать обновления и не может
 * сопрячься по старому ключу, но полная реплика хранилища, которую оно уже
 * скачало, остаётся у него на диске. Настоящая защита при краже — шифрование
 * at-rest (§3.1); отзыв отрезает только от будущего.
 *
 * Себя отозвать нельзя: устройство, на котором открыт UI, — единственное, что
 * у пользователя точно есть в руках. Ядро отвечает `VALIDATION`.
 */
export interface RevokeDeviceRequest {
  device_id: DeviceId
}

/**
 * Изменилось ровно одно устройство — его и возвращаем. Возвращать весь список
 * незачем: в отличие от `set_default_vault`, отзыв не трогает соседей.
 */
export type RevokeDeviceResponse = Device

// ---------------------------------------------------------------------------
// Статус синхронизации (F10, §5.1 · §5.3)
// ---------------------------------------------------------------------------

/**
 * Чем ядро занято прямо сейчас.
 *
 * Это МАШИННОЕ состояние, а не надпись на индикаторе. Шесть видов индикатора из
 * макета («всё сошлось», «рядом никого», «ждут изменения», «идёт обмен», «не
 * удалось», «конфликт») UI выводит сам — из фазы, счётчиков ниже и списка
 * конфликтов. Иначе ядру пришлось бы знать про оформление, а UI — верить ему
 * на слово вместо того, чтобы уметь объяснить, откуда взялась картинка.
 */
export type SyncPhase =
  /** Ничего не происходит: обмена нет, ошибки нет. */
  | 'idle'
  /** Ищем устройства в локальной сети (§5.1, discovery по ARP/mDNS). */
  | 'searching'
  /** Идёт обмен манифестами и диффом (§5.3). */
  | 'syncing'
  /** Последняя попытка сорвалась. Данные целы — не доехал только дифф. */
  | 'error'

/**
 * Что происходит с синхронизацией. Секретов здесь нет и быть не может: это
 * сведения о процессе, а не о содержимом записей.
 *
 * Счётчика конфликтов здесь намеренно НЕТ, хотя индикатор его показывает:
 * список конфликтов приходит отдельной командой (`list_conflicts`), и держать
 * рядом ещё и число значило бы иметь две правды, которые рано или поздно
 * разъедутся. UI складывает картинку из обоих источников.
 */
export interface SyncStatus {
  phase: SyncPhase
  /** Сколько доверенных устройств видно в сети прямо сейчас. */
  peers_online: number
  /** С кем идёт обмен — или с кем он сорвался. `null` вне обмена. */
  peer_name: string | null
  /**
   * Записи, изменённые здесь и ещё не уехавшие. Список, а не счётчик: строка
   * списка помечается «ждёт синхронизации» поимённо (§ «Состояния» макета), а
   * число для шапки — это его длина. Два поля разъехались бы при первой же
   * гонке. Записи локальных секций сюда НЕ попадают: им некуда ехать (§4.2).
   */
  pending_records: RecordId[]
  /** Когда последний обмен успешно закончился. `null` — ни разу. */
  last_sync_at: IsoDateTime | null
  /**
   * Что именно не получилось, словами для пользователя. Непусто только в фазе
   * `error`. Ни адресов, ни ключей, ни путей — как и любое сообщение ядра.
   */
  message: string | null
}

export type GetSyncStatusRequest = Record<string, never>
export type GetSyncStatusResponse = SyncStatus

/**
 * Не ждать следующей автоматической попытки (§ «Состояния»: «следующая попытка
 * через минуту, вручную можно раньше»). Кнопка есть только у состояния
 * «не удалось»: у «рядом никого» повторять нечего, и макет там кнопку запрещает.
 */
export type SyncNowRequest = Record<string, never>
export type SyncNowResponse = SyncStatus

// ---------------------------------------------------------------------------
// Конфликты версий (F11, §5.5)
// ---------------------------------------------------------------------------

/** Чья версия: `local` — эта машина, `remote` — то, что приехало от пира. */
export type ConflictSide = 'local' | 'remote'

/**
 * Поле, по которому версии разошлись. Секретные поля здесь ЕСТЬ — но как имена,
 * а не как значения: «пароли различаются» это факт о записи, а не сам пароль.
 */
export type ConflictField =
  'vault_id' | 'service_name' | 'urls' | 'login' | 'account_label' | SecretField

/**
 * Одна из двух версий записи (§5.5).
 *
 * Секретных значений здесь нет — те же флаги `has_notes` / `has_totp`, что и в
 * `RecordMeta`. «Полный diff» из §5.5 собирается так: метаданные приходят
 * сразу, а секреты открываются разово через `get_conflict_secret` — по тому же
 * явному действию, что и обычный reveal. Иначе один приехавший конфликт
 * выкладывал бы на экран два пароля, которых никто не просил.
 */
export interface ConflictVersion {
  side: ConflictSide
  /** Устройство, на котором сделана эта правка, — «какая версия чья». */
  device_name: string
  version: number
  /** «Какая версия раньше, какая позже» (§5.5) — показывается пользователю. */
  updated_at: IsoDateTime
  /** Секцию тоже могли поменять — а вместе с ней и то, уезжает ли запись (§4.2). */
  vault_id: VaultId
  service_name: string
  urls: string[]
  login: string
  account_label: string | null
  has_notes: boolean
  has_totp: boolean
}

/**
 * Одна и та же запись, независимо изменённая на двух устройствах (§5.5).
 *
 * Разрешение — на уровне ЦЕЛОЙ записи: пользователь выбирает одну версию
 * целиком, а не собирает её по полям. Поэтому здесь ровно две стороны и ни
 * одного «слить автоматически».
 */
export interface RecordConflict {
  record_id: RecordId
  /** Когда ядро обнаружило расхождение — при обмене диффом (§5.3). */
  raised_at: IsoDateTime
  local: ConflictVersion
  remote: ConflictVersion
  /** По каким полям версии расходятся. Значений секретов здесь НЕТ. */
  differing_fields: ConflictField[]
}

export type ListConflictsRequest = Record<string, never>
export type ListConflictsResponse = RecordConflict[]

export interface ResolveConflictRequest {
  record_id: RecordId
  /**
   * Какую версию оставить — СТОРОНОЙ, а не номером версии (в отличие от
   * заготовки в TASKS). Номер здесь не различает: конфликт растёт из общего
   * предка, и обе стороны сплошь и рядом несут одно и то же число — 4 стало 5
   * и здесь, и там. Сторона однозначна и совпадает с тем, что видит человек:
   * он выбирает колонку, а не цифру.
   */
  side: ConflictSide
}

/** Победившая версия — уже как обычная запись, с новым `version`. */
export type ResolveConflictResponse = RecordMeta

/**
 * Открыть одно секретное поле обеих версий (§5.5, «ручная пересборка» в редких
 * случаях). Разово, по явному действию — правила `get_secret` действуют здесь
 * целиком: не в стор, не в localStorage, не в логи.
 */
export interface GetConflictSecretRequest {
  record_id: RecordId
  field: SecretField
}

/**
 * Обе стороны сразу: сравнить одно значение с другим — весь смысл действия.
 * Отдавать их по одному значило бы заставить человека нажать дважды и держать
 * первое значение на экране, пока едет второе.
 */
export interface GetConflictSecretResponse {
  local: string | null
  remote: string | null
}

// ---------------------------------------------------------------------------
// Экспорт / импорт / бэкап (F12, §6.2)
// ---------------------------------------------------------------------------

export type ImportSessionId = string

/**
 * Файл, который ядро СОЗДАЛО на диске.
 *
 * Содержимого здесь нет — только след файла: где лежит, сколько весит, что
 * внутри числом. И это главное решение всего F12: ни CSV, ни бэкап не
 * пересекают границу IPC. Файл целиком собирает и пишет ядро, UI получает путь.
 *
 * Иначе экспорт стал бы четвёртой командой, отдающей открытый текст, — причём
 * не одного поля по нажатию, а ВСЕХ паролей сразу, да ещё и одной строкой в
 * куче JS, которую никто не контролирует. Ровно этого Закон №1 не допускает,
 * и обходить его ради кнопки «сохранить» нельзя.
 *
 * Куда положить файл, решает тоже ядро (папка загрузок / бэкапов): выбор пути
 * — это системный диалог, а он живёт на той стороне вместе с файловой
 * системой. `path` в ответе нужен, чтобы человек нашёл файл и мог его удалить.
 */
export interface ExportFile {
  /** Полный путь — его показывают пользователю: он пойдёт искать файл. */
  path: string
  file_name: string
  size_bytes: number
  /** Сколько записей попало в файл. */
  record_count: number
  created_at: IsoDateTime
  /**
   * Закрыт ли файл мастер-паролем. `false` — открытый текст (CSV), и тогда UI
   * обязан продолжать говорить об этом после экспорта, а не только до него.
   */
  encrypted: boolean
}

/**
 * CSV-экспорт (§6.2) — расшифрованные данные для переезда в другой менеджер.
 *
 * Мастер-пароль обязателен, даже если хранилище уже открыто: одно дело —
 * открытая программа на своём компьютере, другое — файл со всеми паролями
 * открытым текстом. Это разные по цене поступки, и второй подтверждается
 * отдельно.
 */
export interface ExportCsvRequest {
  master_password: string
}

export type ExportCsvResponse = ExportFile

/** Зашифрованный бэкап (§6.2): хранилище целиком под тем же мастер-паролем. */
export interface ExportBackupRequest {
  master_password: string
}

export type ExportBackupResponse = ExportFile

/**
 * Удалить созданный экспорт («Удалить файл сейчас» из макета §3.10).
 *
 * Ядро удаляет ТОЛЬКО те файлы, которые само же и создало в этом сеансе:
 * команда, которой можно передать произвольный путь, была бы способом стереть
 * с диска что угодно чужими руками. Незнакомый путь — `NOT_FOUND`.
 */
export interface DeleteExportRequest {
  path: string
}

/**
 * Восстановление из зашифрованного бэкапа (§6.3, резервный путь).
 *
 * Работает только на устройстве БЕЗ хранилища: восстановление поверх живого
 * хранилища — это слияние двух историй, то есть та же задача, что и синхронизация
 * с конфликтами, а молча перезаписать чужие записи файлом с флешки нельзя.
 * На созданном хранилище ядро отвечает `ALREADY_INITIALIZED`.
 *
 * Пароль здесь — от ФАЙЛА, а не от устройства: бэкап открывается тем
 * мастер-паролем, который был на момент его создания. Он же становится
 * мастер-паролем восстановленного хранилища.
 */
export interface RestoreBackupRequest {
  master_password: string
}

/**
 * Итог восстановления. `null` возвращается, когда человек закрыл окно выбора
 * файла: отказ от выбора — это не ошибка, и ошибкой его показывать не надо.
 */
export interface RestoreBackupResult {
  file_name: string
  /** Сколько записей и секций приехало — числа из файла, а не из UI. */
  records: number
  vaults: number
  initialized_at: IsoDateTime
  /** Восстановленное хранилище сразу открыто: пароль только что вводили. */
  unlocked_at: IsoDateTime
}

export type RestoreBackupResponse = RestoreBackupResult | null

/**
 * Откуда переносим (§3.10 макета). На проводе — только имя источника: как
 * добыть из него файл, объясняет UI (это инструкции про чужие программы,
 * ядру о них знать незачем), а как разобрать — знает ядро.
 */
export type ImportSource = 'chrome' | 'firefox' | '1password' | 'bitwarden' | 'keepass' | 'csv'

export const IMPORT_SOURCES = [
  'chrome',
  'firefox',
  '1password',
  'bitwarden',
  'keepass',
  'csv',
] as const

/** Что будет со строкой файла: новая запись, такая уже есть, пароля нет. */
export type ImportRowStatus = 'new' | 'duplicate' | 'no_password'

/**
 * Строка предпросмотра. Пароля здесь НЕТ — и это то же самое правило, что у
 * `RecordMeta`: показать, что попадёт внутрь, можно и нужно, а выкладывать при
 * этом на экран сотни чужих паролей — нет. В макете это сказано прямым текстом:
 * «прочитан локально · пароли пока не показываем».
 */
export interface ImportPreviewRow {
  site: string
  login: string
  status: ImportRowStatus
}

/**
 * Разобранный файл, ждущий согласия человека.
 *
 * Разобранные строки (вместе с паролями) остаются В ЯДРЕ до `commit_import` —
 * как и ключ сеанса при сопряжении, их незачем гонять через UI. Отказ от
 * импорта (`cancel_import`) заставляет ядро их забыть.
 */
export interface ImportPreview {
  session_id: ImportSessionId
  source: ImportSource
  file_name: string
  total_rows: number
  new_count: number
  duplicate_count: number
  no_password_count: number
  /** Первые строки файла — образец, а не весь файл (`IMPORT_PREVIEW_ROWS`). */
  rows: ImportPreviewRow[]
  /** Имя секции, которую ядро заведёт под этот импорт. */
  target_vault_name: string
}

/** Сколько строк показывает предпросмотр. Политика ЯДРА, здесь — ради текста «и ещё N». */
export const IMPORT_PREVIEW_ROWS = 8

export interface ImportOptions {
  /** Совпал сайт и логин — оставить то, что уже есть в Syncra. */
  skip_duplicates: boolean
  /**
   * Посчитать пароли, повторяющиеся на разных сайтах. Импорт не блокирует:
   * это наследство прошлого менеджера, а не повод не переезжать.
   */
  flag_reused: boolean
}

export interface BeginImportRequest {
  source: ImportSource
}

/** `null` — человек закрыл окно выбора файла. Это не ошибка. */
export type BeginImportResponse = ImportPreview | null

export interface CommitImportRequest {
  session_id: ImportSessionId
  options: ImportOptions
}

export interface ImportResult {
  imported: number
  skipped: number
  /** Куда всё легло: ядро заводит под импорт отдельную секцию. */
  vault: Vault
  /**
   * Исходный файл удалён с диска. Обещание §3.10 макета: «файл разбирается
   * прямо на этом компьютере и удаляется сразу после импорта». Флаг здесь
   * потому, что удалить не всегда удаётся (файл на флешке, права), а обещание
   * должно быть проверяемым, а не декларативным.
   */
  source_file_deleted: boolean
  /** Сколько импортированных паролей повторяется. `0`, если не просили считать. */
  reused_passwords: number
}

export interface CancelImportRequest {
  session_id: ImportSessionId
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
  listVaults: { request: ListVaultsRequest; response: ListVaultsResponse }
  createVault: { request: CreateVaultRequest; response: Vault }
  updateVault: { request: UpdateVaultRequest; response: Vault }
  setVaultSync: { request: SetVaultSyncRequest; response: Vault }
  /**
   * Меняет флаг сразу у двух секций (новая и прежняя), поэтому в ответе — весь
   * список: иначе UI пришлось бы догадываться, с кого флаг сняли.
   */
  setDefaultVault: { request: SetDefaultVaultRequest; response: Vault[] }
  /**
   * Удаление секции НЕ удаляет записи: ядро переносит их в секцию по умолчанию
   * (§4.2 — секция это папка, а не хранилище). В ответе список оставшихся
   * секций; записи после этого стоит перечитать — у них сменился `vault_id`.
   */
  deleteVault: { request: DeleteVaultRequest; response: Vault[] }
  getGeneratorProfile: {
    request: GetGeneratorProfileRequest
    response: GetGeneratorProfileResponse
  }
  saveGeneratorProfile: {
    request: SaveGeneratorProfileRequest
    response: SaveGeneratorProfileResponse
  }
  generatePasswords: { request: GeneratePasswordsRequest; response: GeneratePasswordsResponse }
  getPairingPayload: { request: GetPairingPayloadRequest; response: GetPairingPayloadResponse }
  submitPairedKey: { request: SubmitPairedKeyRequest; response: SubmitPairedKeyResponse }
  /** Записывает второе устройство в доверенные — после сверки слов человеком. */
  confirmPairing: { request: ConfirmPairingRequest; response: ConfirmPairingResponse }
  /** Слова не совпали или передумали: сеанс закрывается, ключ не запоминается. */
  cancelPairing: { request: CancelPairingRequest; response: null }
  /** Доверенные устройства, включая отозванные (F9, §2.3). */
  listDevices: { request: ListDevicesRequest; response: ListDevicesResponse }
  /**
   * Отзыв доступа. Идемпотентен: повторный отзыв возвращает то же устройство с
   * прежним `revoked_at` — момент отзыва это факт, и переписывать его нельзя.
   */
  revokeDevice: { request: RevokeDeviceRequest; response: RevokeDeviceResponse }
  /** Что происходит с синхронизацией прямо сейчас (F10). */
  getSyncStatus: { request: GetSyncStatusRequest; response: GetSyncStatusResponse }
  /** Попробовать обменяться сейчас, не дожидаясь следующей попытки. */
  syncNow: { request: SyncNowRequest; response: SyncNowResponse }
  /** Записи, разошедшиеся на двух устройствах (F11, §5.5). */
  listConflicts: { request: ListConflictsRequest; response: ListConflictsResponse }
  /**
   * Оставить одну версию целиком. Проигравшая версия не склеивается с
   * победившей: §5.5 обещает выбор записи, а не сборку по полям.
   */
  resolveConflict: { request: ResolveConflictRequest; response: ResolveConflictResponse }
  /** Открыть одно секретное поле обеих версий — разово, по нажатию. */
  getConflictSecret: { request: GetConflictSecretRequest; response: GetConflictSecretResponse }
  /**
   * CSV и бэкап (F12, §6.2). Файл собирает и пишет ядро — через границу идёт
   * только след файла на диске, никогда его содержимое.
   */
  exportCsv: { request: ExportCsvRequest; response: ExportCsvResponse }
  exportBackup: { request: ExportBackupRequest; response: ExportBackupResponse }
  /** Удалить созданный экспорт. Ядро удаляет только свои файлы. */
  deleteExport: { request: DeleteExportRequest; response: null }
  /** Восстановление из бэкапа — только на устройстве без хранилища (§6.3). */
  restoreBackup: { request: RestoreBackupRequest; response: RestoreBackupResponse }
  /** Разобрать файл чужого менеджера и показать, что попадёт внутрь. */
  beginImport: { request: BeginImportRequest; response: BeginImportResponse }
  /** Согласие получено — заводим записи. */
  commitImport: { request: CommitImportRequest; response: ImportResult }
  /** Передумали: ядро забывает разобранные строки вместе с их паролями. */
  cancelImport: { request: CancelImportRequest; response: null }
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
  listVaults: 'list_vaults',
  createVault: 'create_vault',
  updateVault: 'update_vault',
  setVaultSync: 'set_vault_sync',
  setDefaultVault: 'set_default_vault',
  deleteVault: 'delete_vault',
  getGeneratorProfile: 'get_generator_profile',
  saveGeneratorProfile: 'save_generator_profile',
  generatePasswords: 'generate_passwords',
  getPairingPayload: 'get_pairing_payload',
  submitPairedKey: 'submit_paired_key',
  confirmPairing: 'confirm_pairing',
  cancelPairing: 'cancel_pairing',
  listDevices: 'list_devices',
  revokeDevice: 'revoke_device',
  getSyncStatus: 'get_sync_status',
  syncNow: 'sync_now',
  listConflicts: 'list_conflicts',
  resolveConflict: 'resolve_conflict',
  getConflictSecret: 'get_conflict_secret',
  exportCsv: 'export_csv',
  exportBackup: 'export_backup',
  deleteExport: 'delete_export',
  restoreBackup: 'restore_backup',
  beginImport: 'begin_import',
  commitImport: 'commit_import',
  cancelImport: 'cancel_import',
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
 * Смена состояния синхронизации (F10).
 *
 * Событие несёт ПОЛНЫЙ статус, а не то, что изменилось. Дельта заставила бы UI
 * собирать состояние по кусочкам и расходиться с ядром при каждом пропущенном
 * событии — а пропустить его легко: подписка живёт не всё время работы окна.
 */
export type SyncStatusEvent = SyncStatus

/**
 * Доверенное устройство появилось в сети (§5.1).
 *
 * Отдельно от `sync_status` намеренно: статус отвечает на вопрос «что сейчас
 * происходит», а это — событие про конкретное устройство, и по нему двигается
 * `Device.last_seen_at` в списке устройств.
 */
export interface PeerFoundEvent {
  device_id: DeviceId
  name: string
  kind: DeviceKind
  found_at: IsoDateTime
}

/**
 * Обратная сторона сопряжения (F8): код показывали ЗДЕСЬ, а прочитали и
 * подтвердили на втором устройстве. Тому, кто держит на экране код, никакая
 * команда об этом не расскажет — он ничего не вызывал, поэтому событие.
 */
export type DevicePairedEvent = PairingResult

/**
 * Приехавшая версия разошлась с местной (F11, §5.5). Событие не требует
 * немедленной реакции: конфликт ждёт в списке столько, сколько нужно.
 */
export type ConflictRaisedEvent = RecordConflict

/** Карта событий ядра. Расширяется по мере задач. */
export interface EventMap {
  unlocked: UnlockedEvent
  locked: LockedEvent
  sync_status: SyncStatusEvent
  peer_found: PeerFoundEvent
  device_paired: DevicePairedEvent
  conflict_raised: ConflictRaisedEvent
}

export type EventName = keyof EventMap

/** Имена событий на проводе (Tauri `listen`). */
export const EVENT_NAMES: Record<EventName, string> = {
  unlocked: 'unlocked',
  locked: 'locked',
  sync_status: 'sync_status',
  peer_found: 'peer_found',
  device_paired: 'device_paired',
  conflict_raised: 'conflict_raised',
}
