import type {
  ChangeMasterPasswordResponse,
  ConflictSide,
  CoreErrorCode,
  Device,
  DeviceId,
  EventMap,
  EventName,
  ExportFile,
  GeneratePasswordsResponse,
  GeneratorProfile,
  GetConflictSecretResponse,
  ImportOptions,
  ImportPreview,
  ImportPreviewRow,
  ImportResult,
  ImportSessionId,
  ImportSource,
  InitVaultResponse,
  IsoDateTime,
  ListRecordsRequest,
  PairingHandshake,
  PairingOffer,
  PairingResult,
  PairingSessionId,
  PinStatus,
  RecordConflict,
  RecordDraft,
  RecordId,
  RecordMeta,
  RecordPatch,
  RecordSecrets,
  RestoreBackupResult,
  SecretField,
  SecuritySettings,
  SecuritySettingsPatch,
  SyncStatus,
  UnlockResponse,
  UnlockWithPinResponse,
  Vault,
  VaultColor,
  VaultId,
  VaultPatch,
  VaultStatus,
} from '../contract'
import {
  IMPORT_PREVIEW_ROWS,
  IMPORT_SOURCES,
  MASTER_PASSWORD_MIN_LENGTH,
  PIN_LENGTH,
  VAULT_COLORS,
  VAULT_NAME_MAX_LENGTH,
} from '../contract'
import { CoreError } from '../errors'
import type { CoreClient, Unsubscribe } from '../ipc'
import { buildConflict, type MockConflictEntry } from './conflicts'
import {
  cloneProfile,
  DEFAULT_GENERATOR_PROFILE,
  generatePasswords,
  validateProfile,
} from './generator'
import {
  buildQrMatrix,
  createManualCode,
  createPairingPayload,
  createThisDevice,
  fingerprintWords,
  normalizePairingInput,
  PAIRING_CODE_TTL_MS,
  peerDevice,
  THIS_DEVICE_NAME,
  unreadableCode,
} from './pairing'
import {
  applySecurityPatch,
  cloneSecuritySettings,
  initialSecuritySettings,
  MOCK_PIN_ATTEMPTS,
} from './security'
import {
  createConflictSeed,
  createDeviceSeed,
  createInitialVault,
  createSeed,
  createVaultSeed,
  MOCK_MASTER_PASSWORD,
  secretFlags,
  type MockSeedEntry,
} from './seed'
import { initialSyncStatus, SYNC_SIMULATION_STEP_MS } from './sync'
import {
  countReusedPasswords,
  createImportFile,
  exportLocation,
  exportSize,
  importHost,
  importRowStatus,
  importVaultName,
  type MockImportEntry,
} from './transfer'

export type { MockConflictEntry } from './conflicts'
export { importHost, importVaultName } from './transfer'
export {
  createVaultSeed,
  MOCK_DEVICE_LAPTOP,
  MOCK_DEVICE_PHONE,
  MOCK_MASTER_PASSWORD,
  MOCK_RECORD_GITHUB,
  MOCK_VAULT_PERSONAL,
  MOCK_VAULT_WORK,
} from './seed'
export { SYNC_FAILURE_MESSAGE, SYNC_SIMULATION_STEP_MS } from './sync'
export { DEFAULT_GENERATOR_PROFILE } from './generator'
export { MOCK_PIN_ATTEMPTS } from './security'
export {
  FINGERPRINT_WORD_COUNT,
  PAIRING_CODE_TTL_MS,
  PAIRING_PAYLOAD_PREFIX,
  normalizePairingInput,
} from './pairing'

/**
 * In-memory фейк-ядро.
 *
 * Держит те же границы, что и настоящее ядро: метаданные и секреты лежат в
 * РАЗНЫХ хранилищах, и `listRecords` физически не может вернуть секрет —
 * ему неоткуда его взять. Это делает Закон №1 проверяемым в тестах.
 *
 * Здесь нет и не должно быть никакой крипты: фейк-ядро не шифрует, оно
 * притворяется. Настоящие шифрование/KDF живут в Rust.
 */

export interface MockCoreOptions {
  /** Задержка каждой команды, мс. Эмулирует реальный IPC. */
  latencyMs?: number
  masterPassword?: string
  seed?: MockSeedEntry[]
  /** Секции, с которыми стартует хранилище (F7). */
  vaults?: Vault[]
  /** Доверенные устройства, с которыми стартует хранилище (F9). */
  devices?: Device[]
  /** Конфликты версий, с которыми стартует хранилище (F11). */
  conflicts?: MockConflictEntry[]
  /**
   * Изображать жизнь синхронизации по таймеру (F10): поиск пиров, обмен,
   * затишье. Для дева; в тестах выключено — иначе состояние менялось бы само
   * посреди проверки. Тесты двигают фазы через `control`.
   */
  simulateSync?: boolean
  /** Источник времени — для детерминированных тестов. */
  now?: () => Date
  /** Начинать разблокированным (удобно для UI-тестов, минующих экран входа). */
  startUnlocked?: boolean
  /**
   * Есть ли уже созданное хранилище. `false` — эмуляция первого запуска (F3):
   * сид не загружается, любая команда до `initVault` падает `NOT_INITIALIZED`.
   */
  initialized?: boolean
  /** Профиль генератора, с которым стартует хранилище (F6). */
  generatorProfile?: GeneratorProfile
  /** Настройки безопасности, с которыми стартует хранилище (F13). */
  securitySettings?: SecuritySettings
  /**
   * PIN быстрого входа на этом устройстве (F13). По умолчанию `null` — не
   * заведён, и экран блокировки спрашивает мастер-пароль.
   *
   * Умолчание именно такое, потому что энролмент PIN нигде в макетах не
   * нарисован: пока не решено, где он живёт, фейк-ядро не должно делать вид,
   * что этот путь пройден. Ветка с клавиатурой достижима через эту опцию или
   * `control.enrollPin()` — это аффорданс для дева и тестов, а не экран.
   */
  pin?: string | null
}

/** Ручки управления фейк-ядром: только для разработки и тестов. */
export interface MockCoreControl {
  /** Уронить следующую команду указанной ошибкой. Можно вызывать несколько раз — очередь. */
  failNext(code: CoreErrorCode, message?: string): void
  setLatency(ms: number): void
  /** Заблокировать «снаружи»: таймаут бездействия, сон системы. */
  forceLock(reason: EventMap['locked']['reason']): void
  isUnlocked(): boolean
  isInitialized(): boolean

  // Синхронизация (F10). Настоящее ядро двигает эти фазы само, увидев пира в
  // сети; фейк-ядру пира взять негде, поэтому фазы двигают снаружи.
  /** Доверенное устройство появилось в сети (§5.1). */
  peerFound(deviceId: DeviceId): void
  /** Устройство ушло из сети: выключили, унесли, сменили сеть. */
  peerLost(deviceId: DeviceId): void
  /** Начать обмен с найденным устройством (§5.3). */
  startSync(deviceId?: DeviceId): void
  /** Закончить обмен: без аргумента — успешно, с текстом — обрывом. */
  finishSync(failure?: string): void
  /**
   * Приехала чужая версия записи и разошлась с местной (F11).
   * Без аргумента берётся сид-конфликт.
   */
  raiseConflict(entry: MockConflictEntry): void
  /**
   * Второе устройство прочитало наш код и подтвердило сопряжение (обратная
   * сторона F8). В процессе тут ровно одно устройство, поэтому событие,
   * которое приходит «с той стороны», приходится вызывать руками.
   */
  pairedByPeer(name?: string): PairingResult

  /**
   * Следующий выбор файла (импорт, восстановление из бэкапа) человек «отменит»
   * — команда вернёт `null`. Настоящее окно выбора открывает ядро, и другого
   * способа проверить эту ветку у UI нет.
   */
  cancelNextFilePick(): void

  /**
   * Завести PIN быстрого входа на этом устройстве (F13).
   *
   * Ручка, а не команда контракта: энролмент PIN не нарисован ни в одном
   * макете, и выдумывать под него экран рано. Здесь она нужна, чтобы ветка с
   * клавиатурой была достижима из дева и тестов.
   */
  enrollPin(pin: string): void
  /** Убрать PIN: снова только мастер-пароль. */
  clearPin(): void

  /** Остановить таймеры фейк-ядра. Нужно тестам и горячей перезагрузке. */
  dispose(): void
  /** Вернуть фейк-ядро к исходному сиду. */
  reset(): void
}

export interface MockCoreClient extends CoreClient {
  readonly control: MockCoreControl
}

const DEFAULT_LATENCY_MS = 90

function delay(ms: number): Promise<void> {
  if (ms <= 0) return Promise.resolve()
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function cloneMeta(meta: RecordMeta): RecordMeta {
  return { ...meta, urls: [...meta.urls] }
}

function cloneSecrets(secrets: RecordSecrets): RecordSecrets {
  return { ...secrets }
}

function cloneVault(vault: Vault): Vault {
  return { ...vault }
}

function cloneDevice(device: Device): Device {
  return { ...device }
}

function cloneConflictEntry(entry: MockConflictEntry): MockConflictEntry {
  return {
    ...entry,
    meta: { ...entry.meta, urls: [...entry.meta.urls] },
    secrets: { ...entry.secrets },
  }
}

function cloneSyncStatus(status: SyncStatus): SyncStatus {
  return { ...status, pending_records: [...status.pending_records] }
}

/** Валидирует и нормализует метаданное поле. */
function requireNonEmpty(value: string, field: string): string {
  const trimmed = value.trim()
  if (trimmed === '') {
    throw new CoreError('VALIDATION', `Поле «${field}» обязательно.`)
  }
  return trimmed
}

/**
 * Валидирует секретное поле, НЕ меняя его. Пароль отдаётся ядру байт в байт:
 * пробел по краям — легитимный символ пароля, обрезать его нельзя.
 */
function requirePresent(value: string, field: string): string {
  if (value.trim() === '') {
    throw new CoreError('VALIDATION', `Поле «${field}» обязательно.`)
  }
  return value
}

export function createMockCoreClient(options: MockCoreOptions = {}): MockCoreClient {
  const now = options.now ?? (() => new Date())
  let latencyMs = options.latencyMs ?? DEFAULT_LATENCY_MS

  const meta = new Map<RecordId, RecordMeta>()
  const secrets = new Map<RecordId, RecordSecrets>()
  /** Порядок секций — порядок создания: сайдбар не должен перетасовываться. */
  let vaults: Vault[] = []
  const failures: CoreError[] = []
  const handlers = new Map<EventName, Set<(payload: never) => void>>()

  // Сопряжение (F8, §2.2). Всё это состояние живёт только в открытом
  // хранилище: замок его сбрасывает вместе с расшифрованными данными.
  /** Код, который это устройство показывает прямо сейчас. */
  let offer: { session_id: PairingSessionId; code: string; expires_at: IsoDateTime } | null = null
  /** Коды, которые это устройство выдавало раньше: по ним отвечаем «истёк». */
  const retiredCodes = new Set<string>()
  /** Прочитанные коды, ждущие сверки слов человеком. */
  const handshakes = new Map<PairingSessionId, PairingHandshake>()
  /** Доверенные устройства, включая отозванные (F9, §2.3). */
  let devices: Device[] = []

  // Синхронизация (F10) и конфликты (F11). Всё это — содержимое открытого
  // хранилища: замок сбрасывает его вместе с расшифрованными данными.
  let sync: SyncStatus = initialSyncStatus()
  /** Приехавшие версии, разошедшиеся с местными (§5.5). */
  let conflicts: MockConflictEntry[] = []

  // Экспорт и импорт (F12, §6.2).
  /** Файлы, которые ядро создало и ещё не удалило: только их можно удалить. */
  let exportedFiles: ExportFile[] = []
  /**
   * Разобранные файлы, ждущие согласия человека. Здесь лежат чужие пароли
   * открытым текстом — ровно поэтому они и лежат ЗДЕСЬ, а не в UI, и исчезают
   * вместе с открытым хранилищем.
   */
  const importSessions = new Map<
    ImportSessionId,
    { source: ImportSource; file_name: string; entries: MockImportEntry[] }
  >()
  /** Ручка тестов: следующий выбор файла человек «отменит». */
  let cancelNextPick = false
  let simulation: ReturnType<typeof setInterval> | null = null
  let simulationStep = 0

  let masterPassword = options.masterPassword ?? MOCK_MASTER_PASSWORD
  let initialized = options.initialized !== false
  let unlocked = false
  let unlockedAt: IsoDateTime | null = null
  /** Профиль генератора живёт в хранилище рядом с записями (F6). */
  let generatorProfile: GeneratorProfile = cloneProfile(
    options.generatorProfile ?? DEFAULT_GENERATOR_PROFILE,
  )
  /** Таймауты замка, буфера и показа секрета (F13). */
  let securitySettings: SecuritySettings = cloneSecuritySettings(
    options.securitySettings ?? initialSecuritySettings(),
  )
  /**
   * PIN быстрого входа. Свойство УСТРОЙСТВА, а не хранилища: он не уезжает на
   * другие устройства и не участвует в бэкапе.
   */
  let pin: string | null = options.pin ?? null
  let pinAttemptsLeft = MOCK_PIN_ATTEMPTS

  function timestamp(): IsoDateTime {
    return now().toISOString()
  }

  function loadSeed(): void {
    meta.clear()
    secrets.clear()
    vaults = []
    devices = []
    conflicts = []
    sync = initialSyncStatus()
    forgetPairing()
    forgetImports()
    exportedFiles = []
    // Свежесозданное хранилище пустое: сид — это «данные, которые уже были».
    if (!initialized) return

    conflicts = (options.conflicts ?? createConflictSeed()).map(cloneConflictEntry)
    devices = (options.devices ?? createDeviceSeed(now())).map(cloneDevice)
    vaults = (options.vaults ?? createVaultSeed()).map(cloneVault)
    for (const entry of options.seed ?? createSeed()) {
      const alive = entry.meta.deleted_at === null
      // У надгробия нет полезной нагрузки — ни секретов, ни флагов о них.
      const flags = alive ? secretFlags(entry.secrets) : { has_notes: false, has_totp: false }

      meta.set(entry.meta.record_id, cloneMeta({ ...entry.meta, ...flags }))
      if (alive) secrets.set(entry.meta.record_id, cloneSecrets(entry.secrets))
    }
  }

  function emit<E extends EventName>(event: E, payload: EventMap[E]): void {
    for (const handler of handlers.get(event) ?? []) {
      ;(handler as (payload: EventMap[E]) => void)(payload)
    }
  }

  // -------------------------------------------------------------------------
  // Синхронизация (F10)
  //
  // Фейк-ядро не ходит в сеть: оно только изображает то, что видно UI. Фазы
  // двигаются снаружи (`control`) или демо-таймером — но состояние при этом
  // остаётся согласованным, иначе индикатор нечего было бы проверять.
  // -------------------------------------------------------------------------

  /** Кто из доверенных устройств виден в сети прямо сейчас. */
  const online = new Set<DeviceId>()

  function emitSync(): void {
    emit('sync_status', cloneSyncStatus(sync))
  }

  /** Изменить статус и рассказать об этом. `peers_online` всегда считается. */
  function setSync(patch: Partial<SyncStatus>): void {
    sync = { ...sync, ...patch, peers_online: online.size }
    emitSync()
  }

  /**
   * Запись изменилась и ещё не уехала.
   *
   * Записи локальных секций сюда не попадают никогда: выключенная
   * синхронизация означает «никуда» (§4.2), и пометка «ждёт синхронизации»
   * обещала бы отправку, которой не будет.
   */
  function markPending(record: RecordMeta): void {
    const vault = vaults.find((item) => item.vault_id === record.vault_id)
    if (vault === undefined || !vault.sync) return
    if (sync.pending_records.includes(record.record_id)) return
    setSync({ pending_records: [...sync.pending_records, record.record_id] })
  }

  /** Сколько живых записей уехало бы на второе устройство (§4.2). */
  function syncableRecordCount(): number {
    const syncing = new Set(vaults.filter((vault) => vault.sync).map((vault) => vault.vault_id))
    return [...meta.values()].filter(
      (record) => record.deleted_at === null && syncing.has(record.vault_id),
    ).length
  }

  function doPeerFound(deviceId: DeviceId): void {
    const device = devices.find((item) => item.device_id === deviceId)
    // Отозванное устройство discovery в сети найти может, а вот доверенным
    // пиром оно больше не считается (§2.3) — в счётчик оно не идёт.
    if (device === undefined || device.is_this_device || device.revoked_at !== null) return

    const foundAt = timestamp()
    devices = devices.map((item) =>
      item.device_id === deviceId ? { ...item, last_seen_at: foundAt } : item,
    )
    online.add(deviceId)
    emit('peer_found', {
      device_id: deviceId,
      name: device.name,
      kind: device.kind,
      found_at: foundAt,
    })
    setSync({})
  }

  function doPeerLost(deviceId: DeviceId): void {
    if (online.delete(deviceId)) setSync({})
  }

  function doStartSync(deviceId?: DeviceId): void {
    const peer =
      deviceId === undefined
        ? devices.find((item) => online.has(item.device_id))
        : devices.find((item) => item.device_id === deviceId)
    setSync({ phase: 'syncing', peer_name: peer?.name ?? null, message: null })
  }

  function doFinishSync(failure?: string): void {
    if (failure !== undefined) {
      // Ошибка не трогает `pending_records`: изменения никуда не делись и
      // поедут со следующей попыткой. «Не удалось» — про связь, не про данные.
      setSync({ phase: 'error', message: failure })
      return
    }

    setSync({
      phase: 'idle',
      peer_name: null,
      message: null,
      pending_records: [],
      last_sync_at: timestamp(),
    })
  }

  /** Демо-цикл для дева: ищем → нашли → обменялись → тишина. */
  function simulationTick(): void {
    if (!unlocked) return

    const peer = devices.find((item) => !item.is_this_device && item.revoked_at === null)
    const step = simulationStep % 4
    simulationStep += 1

    if (step === 0) setSync({ phase: 'searching', peer_name: null, message: null })
    else if (step === 1 && peer !== undefined) doPeerFound(peer.device_id)
    else if (step === 2) doStartSync(peer?.device_id)
    else doFinishSync()
  }

  function startSimulation(): void {
    if (options.simulateSync !== true || simulation !== null) return
    simulationStep = 0
    simulation = setInterval(simulationTick, SYNC_SIMULATION_STEP_MS)
  }

  function stopSimulation(): void {
    if (simulation !== null) clearInterval(simulation)
    simulation = null
  }

  // -------------------------------------------------------------------------
  // Конфликты версий (F11, §5.5)
  // -------------------------------------------------------------------------

  function conflictFor(recordId: RecordId): MockConflictEntry {
    const found = conflicts.find((entry) => entry.record_id === recordId)
    if (!found) throw new CoreError('NOT_FOUND', 'Конфликт версий не найден.')
    return found
  }

  function liveSecrets(recordId: RecordId): RecordSecrets {
    const found = secrets.get(recordId)
    if (!found) throw new CoreError('NOT_FOUND', 'Запись не найдена.')
    return found
  }

  /** Общий пролог команды: задержка, впрыск ошибки, проверка замка. */
  async function enter(gate: { requiresUnlock: boolean; requiresInit?: boolean }): Promise<void> {
    await delay(latencyMs)

    const injected = failures.shift()
    if (injected) throw injected

    if ((gate.requiresInit ?? gate.requiresUnlock) && !initialized) {
      throw new CoreError('NOT_INITIALIZED', 'Хранилище ещё не создано.')
    }

    if (gate.requiresUnlock && !unlocked) {
      throw new CoreError('LOCKED', 'Хранилище заблокировано.')
    }
  }

  // -------------------------------------------------------------------------
  // Секции (F7, §4.2)
  // -------------------------------------------------------------------------

  function findVault(vaultId: VaultId): Vault {
    const found = vaults.find((vault) => vault.vault_id === vaultId)
    if (!found) throw new CoreError('NOT_FOUND', 'Секция не найдена.')
    return found
  }

  /** Секция по умолчанию есть всегда: иначе новую запись некуда положить. */
  function defaultVault(): Vault {
    const found = vaults.find((vault) => vault.is_default) ?? vaults[0]
    if (!found) throw new CoreError('INTERNAL', 'В хранилище не осталось ни одной секции.')
    return found
  }

  function validVaultName(name: string): string {
    const trimmed = name.trim()
    if (trimmed === '') throw new CoreError('VALIDATION', 'У секции должно быть имя.')
    if (trimmed.length > VAULT_NAME_MAX_LENGTH) {
      throw new CoreError('VALIDATION', `Имя секции длиннее ${VAULT_NAME_MAX_LENGTH} символов.`)
    }
    return trimmed
  }

  function validVaultColor(color: VaultColor): VaultColor {
    if (!VAULT_COLORS.includes(color)) {
      throw new CoreError('VALIDATION', 'Неизвестный цвет метки секции.')
    }
    return color
  }

  function liveRecord(recordId: RecordId): RecordMeta {
    const found = meta.get(recordId)
    if (!found || found.deleted_at !== null) {
      throw new CoreError('NOT_FOUND', 'Запись не найдена.')
    }
    return found
  }

  function doUnlock(): IsoDateTime {
    unlocked = true
    unlockedAt = timestamp()
    startSimulation()
    emit('unlocked', { unlocked_at: unlockedAt })
    return unlockedAt
  }

  function doLock(reason: EventMap['locked']['reason']): void {
    if (!unlocked) return
    unlocked = false
    unlockedAt = null
    // Незавершённое сопряжение не переживает замок: код на экране даёт право
    // забрать копию хранилища, а закрытое хранилище такого права не выдаёт.
    forgetPairing()
    // Разобранный, но не подтверждённый импорт — тоже: это чужие пароли,
    // которые ещё никуда не легли. Созданные файлы при этом остаются на диске:
    // замок не умеет их стереть, и делать вид, что умеет, не будет.
    forgetImports()
    // Закрытое хранилище не синхронизируется: связи с пирами больше нет.
    // `pending_records` при этом остаются — изменения никуда не делись и
    // поедут, когда хранилище откроют; конфликты тоже ждут в хранилище.
    stopSimulation()
    online.clear()
    sync = { ...sync, phase: 'idle', peers_online: 0, peer_name: null, message: null }
    emit('locked', { locked_at: timestamp(), reason })
  }

  /** Забыть код и все начатые сеансы сопряжения. */
  function forgetPairing(): void {
    offer = null
    retiredCodes.clear()
    handshakes.clear()
  }

  /**
   * Забыть разобранные файлы. В них лежат чужие пароли открытым текстом, и
   * закрытое хранилище — не то место, где им можно продолжать лежать.
   */
  function forgetImports(): void {
    importSessions.clear()
  }

  function expired(at: IsoDateTime): boolean {
    return Date.parse(at) <= now().getTime()
  }

  // -------------------------------------------------------------------------
  // Экспорт / импорт / бэкап (F12, §6.2)
  // -------------------------------------------------------------------------

  /**
   * Экспорт требует мастер-пароль, даже когда хранилище открыто (§3.10 макета).
   * Открытая программа и файл со всеми паролями на диске — поступки разной
   * цены, и второй подтверждается отдельно.
   */
  function requireMasterPassword(input: string): void {
    if (input !== masterPassword) {
      throw new CoreError('INVALID_MASTER_PASSWORD', 'Неверный мастер-пароль.')
    }
  }

  /** Состояние быстрого входа для `getVaultStatus` (F13). */
  function pinStatus(): PinStatus {
    return {
      enrolled: pin !== null,
      attempts_left: pin === null ? null : pinAttemptsLeft,
    }
  }

  /** Выключить быстрый вход: попытки кончились или человек попросил сам. */
  function forgetPin(): void {
    pin = null
    pinAttemptsLeft = MOCK_PIN_ATTEMPTS
  }

  function liveRecords(): RecordMeta[] {
    return [...meta.values()].filter((record) => record.deleted_at === null)
  }

  function registerExport(kind: 'csv' | 'backup', records: number): ExportFile {
    const at = now()
    const { path, file_name } = exportLocation(kind, at)
    const file: ExportFile = {
      path,
      file_name,
      size_bytes: exportSize(kind, records),
      record_count: records,
      created_at: at.toISOString(),
      encrypted: kind === 'backup',
    }

    // Повторный экспорт в тот же день перезаписывает файл, а не плодит второй.
    exportedFiles = [...exportedFiles.filter((item) => item.path !== path), file]
    return { ...file }
  }

  /**
   * Пары «адрес + логин», которые в хранилище уже есть. По ним считается
   * дубликат при импорте — по адресу и логину (§4.4), а не по имени сервиса.
   */
  function knownPairs(): Set<string> {
    const pairs = new Set<string>()
    for (const record of liveRecords()) {
      const login = record.login.trim().toLowerCase()
      for (const url of record.urls) pairs.add(`${importHost(url)} ${login}`)
    }
    return pairs
  }

  /**
   * Секция под импорт. Отдельная намеренно (§3.10 макета: «разберёте потом, не
   * смешивая с текущим»): 300 чужих записей, высыпанных в «Личное», — это не
   * перенос, а беспорядок. Второй импорт в тот же день ложится туда же.
   */
  function importVault(): Vault {
    const name = importVaultName(now())
    const existing = vaults.find((vault) => vault.name === name)
    if (existing) return existing

    const vault: Vault = {
      vault_id: crypto.randomUUID(),
      name,
      color: 'mint',
      // Импортированное синхронизируется, как и любая новая секция.
      sync: true,
      is_default: false,
      created_at: timestamp(),
    }
    vaults = [...vaults, vault]
    return vault
  }

  /** `figma.com` → `Figma`. Имя сервиса — подпись для человека, не ключ (§4.1). */
  function serviceNameFromHost(host: string): string {
    const label = host.split('.')[0] ?? host
    return label === '' ? host : label[0]!.toUpperCase() + label.slice(1)
  }

  loadSeed()
  if (options.startUnlocked && initialized) {
    unlocked = true
    unlockedAt = timestamp()
    startSimulation()
  }

  const client: MockCoreClient = {
    async getVaultStatus(): Promise<VaultStatus> {
      // Статус доступен всегда: с него начинается запуск UI.
      await enter({ requiresUnlock: false, requiresInit: false })
      return { initialized, unlocked, unlocked_at: unlockedAt, pin: pinStatus() }
    },

    async initVault(masterPasswordInput: string): Promise<InitVaultResponse> {
      await enter({ requiresUnlock: false, requiresInit: false })

      if (initialized) {
        throw new CoreError('ALREADY_INITIALIZED', 'Хранилище на этом устройстве уже создано.')
      }
      if (masterPasswordInput.length < MASTER_PASSWORD_MIN_LENGTH) {
        throw new CoreError(
          'VALIDATION',
          `Мастер-пароль короче ${MASTER_PASSWORD_MIN_LENGTH} символов.`,
        )
      }

      // В настоящем ядре здесь генерация ключей и KDF. Фейк-ядро просто
      // запоминает пароль: крипты во фронте нет и в моке её быть не должно.
      masterPassword = masterPasswordInput
      initialized = true
      // Только что созданное хранилище пустое — записи заводит уже пользователь.
      meta.clear()
      secrets.clear()
      generatorProfile = cloneProfile(DEFAULT_GENERATOR_PROFILE)
      securitySettings = initialSecuritySettings()
      // Быстрого входа на новом хранилище нет: PIN — это то, что настраивают
      // потом, и подставлять его за пользователя нельзя.
      forgetPin()

      const initializedAt = timestamp()
      // Одна секция по умолчанию: выбирать между секциями до того, как они
      // заведены, не из чего.
      vaults = [createInitialVault(initializedAt)]
      // Устройство ровно одно — это. Второе появится через сопряжение (F8).
      devices = [createThisDevice(initializedAt)]
      forgetPairing()
      return { initialized_at: initializedAt, unlocked_at: doUnlock() }
    },

    async unlock(masterPasswordInput: string): Promise<UnlockResponse> {
      await enter({ requiresUnlock: false, requiresInit: true })

      if (masterPasswordInput !== masterPassword) {
        throw new CoreError('INVALID_MASTER_PASSWORD', 'Неверный мастер-пароль.')
      }

      // Мастер-пароль подтверждён — счётчик попыток PIN снова полный: человек
      // доказал, что хранилище его, и наказывать за забытый PIN нечем.
      pinAttemptsLeft = MOCK_PIN_ATTEMPTS
      return { unlocked_at: doUnlock() }
    },

    async unlockWithPin(pinInput: string): Promise<UnlockWithPinResponse> {
      await enter({ requiresUnlock: false, requiresInit: true })

      // Не та форма запроса — виноват UI, и это настоящая ошибка.
      if (!new RegExp(`^\\d{${PIN_LENGTH}}$`).test(pinInput)) {
        throw new CoreError('VALIDATION', `PIN — это ${PIN_LENGTH} цифры.`)
      }
      if (pin === null) {
        throw new CoreError('VALIDATION', 'Быстрый вход на этом устройстве не настроен.')
      }

      if (pinInput !== pin) {
        pinAttemptsLeft -= 1
        const exhausted = pinAttemptsLeft <= 0
        // Попытки кончились — быстрый вход выключается целиком: дальше только
        // мастер-пароль, и UI обязан сказать об этом словами.
        if (exhausted) forgetPin()
        return {
          ok: false,
          attempts_left: exhausted ? 0 : pinAttemptsLeft,
          pin_disabled: exhausted,
        }
      }

      pinAttemptsLeft = MOCK_PIN_ATTEMPTS
      return { ok: true, unlocked_at: doUnlock() }
    },

    async lock(): Promise<void> {
      await enter({ requiresUnlock: false })
      doLock('manual')
    },

    async changeMasterPassword(
      currentInput: string,
      nextInput: string,
    ): Promise<ChangeMasterPasswordResponse> {
      // За замком: перешифровать хранилище можно только открыв его.
      await enter({ requiresUnlock: true })

      requireMasterPassword(currentInput)

      if (nextInput.length < MASTER_PASSWORD_MIN_LENGTH) {
        throw new CoreError(
          'VALIDATION',
          `Мастер-пароль короче ${MASTER_PASSWORD_MIN_LENGTH} символов.`,
        )
      }
      if (nextInput === currentInput) {
        throw new CoreError('VALIDATION', 'Новый мастер-пароль совпадает с текущим.')
      }

      // В настоящем ядре здесь перешифровка хранилища новым ключом. Фейк-ядро
      // просто запоминает пароль — крипты во фронте нет и в моке её быть не должно.
      masterPassword = nextInput
      // Быстрый вход отпирал ключ, выведенный из СТАРОГО пароля: после
      // перешифровки он больше ничего не отпирает и должен быть заведён заново.
      forgetPin()

      // Хранилище остаётся открытым: пароль только что подтвердили.
      return {
        changed_at: timestamp(),
        devices_to_update: devices.filter(
          (device) => !device.is_this_device && device.revoked_at === null,
        ).length,
      }
    },

    async listRecords(request: ListRecordsRequest = {}): Promise<RecordMeta[]> {
      await enter({ requiresUnlock: true })

      return [...meta.values()]
        .filter((record) => request.include_deleted === true || record.deleted_at === null)
        .filter((record) => request.vault_id === undefined || record.vault_id === request.vault_id)
        .sort(
          (a, b) => a.service_name.localeCompare(b.service_name) || a.login.localeCompare(b.login),
        )
        .map(cloneMeta)
    },

    async getSecret(recordId: RecordId): Promise<RecordSecrets> {
      await enter({ requiresUnlock: true })

      liveRecord(recordId)
      const found = secrets.get(recordId)
      if (!found) throw new CoreError('NOT_FOUND', 'Запись не найдена.')

      // Копия: то, что UI сделает со значением, не должно менять «хранилище».
      return cloneSecrets(found)
    },

    async createRecord(draft: RecordDraft): Promise<RecordMeta> {
      await enter({ requiresUnlock: true })

      const serviceName = requireNonEmpty(draft.service_name, 'Сервис')
      const login = requireNonEmpty(draft.login, 'Логин')
      const password = requirePresent(draft.password, 'Пароль')
      // Секцию проверяем до записи: чужой `vault_id` из UI не должен создать
      // запись, которой нет ни в одной секции.
      const vault = draft.vault_id === undefined ? defaultVault() : findVault(draft.vault_id)

      const createdAt = timestamp()
      const newSecrets: RecordSecrets = {
        password,
        notes: draft.notes ?? null,
        totp_secret: draft.totp_secret ?? null,
      }
      // ID генерирует ядро, а не UI. Здесь это делает фейк-ядро в роли ядра.
      const record: RecordMeta = {
        record_id: crypto.randomUUID(),
        vault_id: vault.vault_id,
        service_name: serviceName,
        urls: [...draft.urls],
        login,
        account_label: draft.account_label ?? null,
        ...secretFlags(newSecrets),
        version: 1,
        created_at: createdAt,
        updated_at: createdAt,
        password_updated_at: createdAt,
        deleted_at: null,
      }

      meta.set(record.record_id, record)
      secrets.set(record.record_id, newSecrets)
      // Новая запись ещё нигде, кроме этого устройства (F10).
      markPending(record)

      return cloneMeta(record)
    },

    async updateRecord(recordId: RecordId, patch: RecordPatch): Promise<RecordMeta> {
      await enter({ requiresUnlock: true })

      const current = liveRecord(recordId)
      const currentSecrets = secrets.get(recordId)
      if (!currentSecrets) throw new CoreError('NOT_FOUND', 'Запись не найдена.')

      const changedAt = timestamp()
      const nextPassword =
        patch.password === undefined
          ? currentSecrets.password
          : requirePresent(patch.password, 'Пароль')
      const passwordChanged = nextPassword !== currentSecrets.password

      const nextSecrets: RecordSecrets = {
        password: nextPassword,
        notes: patch.notes === undefined ? currentSecrets.notes : patch.notes,
        totp_secret:
          patch.totp_secret === undefined ? currentSecrets.totp_secret : patch.totp_secret,
      }

      const next: RecordMeta = {
        ...current,
        vault_id:
          patch.vault_id === undefined ? current.vault_id : findVault(patch.vault_id).vault_id,
        service_name:
          patch.service_name === undefined
            ? current.service_name
            : requireNonEmpty(patch.service_name, 'Сервис'),
        urls: patch.urls === undefined ? current.urls : [...patch.urls],
        login: patch.login === undefined ? current.login : requireNonEmpty(patch.login, 'Логин'),
        account_label:
          patch.account_label === undefined ? current.account_label : patch.account_label,
        ...secretFlags(nextSecrets),
        version: current.version + 1,
        updated_at: changedAt,
        password_updated_at: passwordChanged ? changedAt : current.password_updated_at,
      }

      meta.set(recordId, next)
      secrets.set(recordId, nextSecrets)
      markPending(next)

      return cloneMeta(next)
    },

    async deleteRecord(recordId: RecordId): Promise<RecordMeta> {
      await enter({ requiresUnlock: true })

      const current = liveRecord(recordId)
      const deletedAt = timestamp()
      const tombstone: RecordMeta = {
        ...current,
        // Надгробие не хранит секретов — значит, и «есть заметка» про него
        // больше не правда.
        has_notes: false,
        has_totp: false,
        version: current.version + 1,
        updated_at: deletedAt,
        deleted_at: deletedAt,
      }

      meta.set(recordId, tombstone)
      // Надгробие не хранит секретов (§5.4).
      secrets.delete(recordId)
      // Удаление так же важно, как добавление: надгробие тоже должно доехать.
      markPending(tombstone)
      // Спорить о версиях удалённой записи больше не о чем.
      conflicts = conflicts.filter((entry) => entry.record_id !== recordId)

      return cloneMeta(tombstone)
    },

    async listVaults(): Promise<Vault[]> {
      // Состав секций — содержимое хранилища: за замком его не показываем.
      await enter({ requiresUnlock: true })
      return vaults.map(cloneVault)
    },

    async createVault(name: string, color: VaultColor): Promise<Vault> {
      await enter({ requiresUnlock: true })

      const vault: Vault = {
        vault_id: crypto.randomUUID(),
        name: validVaultName(name),
        color: validVaultColor(color),
        // Новая секция синхронизируется: продукт про синхронизацию, и молча
        // оставлять записи на одном устройстве было бы сюрпризом. Выключить —
        // тумблером, осознанно.
        sync: true,
        is_default: false,
        created_at: timestamp(),
      }

      vaults = [...vaults, vault]
      return cloneVault(vault)
    },

    async updateVault(vaultId: VaultId, patch: VaultPatch): Promise<Vault> {
      await enter({ requiresUnlock: true })

      const current = findVault(vaultId)
      const next: Vault = {
        ...current,
        name: patch.name === undefined ? current.name : validVaultName(patch.name),
        color: patch.color === undefined ? current.color : validVaultColor(patch.color),
      }

      vaults = vaults.map((vault) => (vault.vault_id === vaultId ? next : vault))
      return cloneVault(next)
    },

    async setVaultSync(vaultId: VaultId, sync: boolean): Promise<Vault> {
      await enter({ requiresUnlock: true })

      const next: Vault = { ...findVault(vaultId), sync }
      vaults = vaults.map((vault) => (vault.vault_id === vaultId ? next : vault))
      return cloneVault(next)
    },

    async setDefaultVault(vaultId: VaultId): Promise<Vault[]> {
      await enter({ requiresUnlock: true })

      findVault(vaultId)
      // Ровно одна секция помечена: флаг снимается со всех остальных сразу.
      vaults = vaults.map((vault) => ({ ...vault, is_default: vault.vault_id === vaultId }))
      return vaults.map(cloneVault)
    },

    async deleteVault(vaultId: VaultId): Promise<Vault[]> {
      await enter({ requiresUnlock: true })

      const doomed = findVault(vaultId)
      if (doomed.is_default) {
        throw new CoreError(
          'VALIDATION',
          'Секция по умолчанию не удаляется: новым записям нужно куда-то попадать.',
        )
      }

      vaults = vaults.filter((vault) => vault.vault_id !== vaultId)

      // Записи НЕ удаляются вместе с секцией (§4.2): они переезжают в секцию
      // по умолчанию. Смена секции — обычное изменение записи, поэтому версия
      // растёт: на другом устройстве переезд должен быть виден как правка.
      const home = defaultVault().vault_id
      const movedAt = timestamp()
      for (const [recordId, record] of meta) {
        // Надгробие не переносим: переезжать нечему, а лишняя версия у него
        // только добавила бы работы синхронизации (§5.4).
        if (record.vault_id !== vaultId || record.deleted_at !== null) continue
        meta.set(recordId, {
          ...record,
          vault_id: home,
          version: record.version + 1,
          updated_at: movedAt,
        })
      }

      return vaults.map(cloneVault)
    },

    async getGeneratorProfile(): Promise<GeneratorProfile> {
      // Профиль — часть содержимого хранилища, поэтому за замком: на закрытом
      // хранилище рассказывать, какими правилами пользуется владелец, незачем.
      await enter({ requiresUnlock: true })
      return cloneProfile(generatorProfile)
    },

    async saveGeneratorProfile(profile: GeneratorProfile): Promise<GeneratorProfile> {
      await enter({ requiresUnlock: true })

      generatorProfile = validateProfile(profile)
      return cloneProfile(generatorProfile)
    },

    async getSecuritySettings(): Promise<SecuritySettings> {
      // За замком, как и профиль генератора: рассказывать про настройки защиты
      // закрытого хранилища незачем.
      await enter({ requiresUnlock: true })
      return cloneSecuritySettings(securitySettings)
    },

    async saveSecuritySettings(patch: SecuritySettingsPatch): Promise<SecuritySettings> {
      await enter({ requiresUnlock: true })

      securitySettings = applySecurityPatch(securitySettings, patch)
      return cloneSecuritySettings(securitySettings)
    },

    async generatePasswords(count: number, profile?: GeneratorProfile) {
      await enter({ requiresUnlock: true })

      // Разовые правила (предпросмотр на экране настроек) проверяются так же
      // строго, как сохраняемые, но профиль в хранилище НЕ меняют.
      const rules = profile === undefined ? generatorProfile : validateProfile(profile)
      return generatePasswords(rules, count) satisfies GeneratePasswordsResponse
    },

    // -------------------------------------------------------------------------
    // Сопряжение устройств (F8, §2.2)
    // -------------------------------------------------------------------------

    async getPairingPayload(): Promise<PairingOffer> {
      // Код даёт право забрать копию хранилища — на закрытом хранилище такого
      // права нет, поэтому команда за замком.
      await enter({ requiresUnlock: true })

      // Каждый запрос — НОВЫЙ код: одноразовый ключ на то и одноразовый.
      // Прежний сразу перестаёт действовать, а не доживает свой срок.
      if (offer) retiredCodes.add(offer.code)

      const code = createManualCode()
      const payload = createPairingPayload(code)
      offer = {
        session_id: crypto.randomUUID(),
        code,
        expires_at: new Date(now().getTime() + PAIRING_CODE_TTL_MS).toISOString(),
      }

      return {
        session_id: offer.session_id,
        // UI получает картинку кода, а не пейлоад: одноразовому ключу сеанса
        // незачем существовать ещё и JS-строкой.
        qr: buildQrMatrix(payload),
        manual_code: code,
        expires_at: offer.expires_at,
      }
    },

    async submitPairedKey(payload: string): Promise<PairingHandshake> {
      await enter({ requiresUnlock: true })

      const code = normalizePairingInput(payload)
      if (code === null) throw unreadableCode()

      // Свой же код, который уже не действует, — самая частая ошибка: человек
      // читает со второго экрана устаревший QR.
      if (
        retiredCodes.has(code) ||
        (offer !== null && offer.code === code && expired(offer.expires_at))
      ) {
        throw new CoreError(
          'PAIRING_EXPIRED',
          'Этот код больше не действует. Покажите новый на втором устройстве.',
        )
      }
      if (offer !== null && offer.code === code) {
        throw new CoreError(
          'VALIDATION',
          'Это код этого же устройства. Нужен код второго — с его экрана.',
        )
      }

      const peer = peerDevice(code)
      const handshake: PairingHandshake = {
        session_id: crypto.randomUUID(),
        peer_name: peer.name,
        peer_kind: peer.kind,
        // Слова считаются из кода, поэтому на обоих экранах выходят
        // одинаковыми — в этом весь смысл сверки (§2.2).
        fingerprint_words: fingerprintWords(code),
        expires_at: new Date(now().getTime() + PAIRING_CODE_TTL_MS).toISOString(),
      }

      // Устройство ещё НЕ доверенное: сначала человек сверяет слова.
      handshakes.set(handshake.session_id, handshake)
      return { ...handshake, fingerprint_words: [...handshake.fingerprint_words] }
    },

    async confirmPairing(sessionId: PairingSessionId): Promise<PairingResult> {
      await enter({ requiresUnlock: true })

      const handshake = handshakes.get(sessionId)
      if (!handshake) throw new CoreError('NOT_FOUND', 'Сеанс сопряжения не найден.')

      handshakes.delete(sessionId)
      if (expired(handshake.expires_at)) {
        throw new CoreError('PAIRING_EXPIRED', 'Сеанс сопряжения истёк. Начните заново.')
      }

      const pairedAt = timestamp()
      const device: Device = {
        device_id: crypto.randomUUID(),
        name: handshake.peer_name,
        kind: handshake.peer_kind,
        is_this_device: false,
        paired_at: pairedAt,
        last_seen_at: pairedAt,
        revoked_at: null,
      }
      devices = [...devices, device]

      // Уезжают только записи синхронизируемых секций: выключенная
      // синхронизация означает «никуда», в том числе на новое устройство (§4.2).
      const transferred = syncableRecordCount()

      // Событие `device_paired` отсюда НЕ шлётся: его получает та сторона,
      // которая показывала код и ничего не вызывала. Здесь результат и так
      // возвращается ответом на команду — второе уведомление было бы эхом.
      return {
        device: { ...device },
        records_transferred: transferred,
        // Настоящее ядро измеряет перенос; фейк-ядро его не делает и честно
        // считает время по объёму, а не выдумывает красивую цифру.
        duration_ms: 200 + transferred * 9,
      }
    },

    async cancelPairing(sessionId: PairingSessionId): Promise<void> {
      await enter({ requiresUnlock: true })
      // Идемпотентно: отменять уже отменённое — не ошибка, а норма для кнопки
      // «Отмена», по которой можно нажать дважды.
      handshakes.delete(sessionId)
    },

    // -------------------------------------------------------------------------
    // Доверенные устройства и отзыв (F9, §2.3)
    // -------------------------------------------------------------------------

    async listDevices(): Promise<Device[]> {
      // Список доверенных устройств — содержимое хранилища: на закрытом
      // хранилище рассказывать, что у владельца ещё есть телефон, незачем.
      await enter({ requiresUnlock: true })
      // Отозванные из списка не исчезают: отзыв — факт истории, а не удаление.
      return devices.map(cloneDevice)
    },

    async revokeDevice(deviceId: DeviceId): Promise<Device> {
      await enter({ requiresUnlock: true })

      const found = devices.find((device) => device.device_id === deviceId)
      if (!found) throw new CoreError('NOT_FOUND', 'Устройство не найдено.')

      if (found.is_this_device) {
        throw new CoreError(
          'VALIDATION',
          'Нельзя отозвать устройство, на котором вы сейчас работаете.',
        )
      }

      // Идемпотентно, и `revoked_at` не переписывается: момент отзыва — факт,
      // от которого считается, что устройство успело догнать, а что нет.
      if (found.revoked_at !== null) return cloneDevice(found)

      const next: Device = { ...found, revoked_at: timestamp() }
      devices = devices.map((device) => (device.device_id === deviceId ? next : device))

      // Отозванное устройство перестаёт быть пиром сразу, а не после того, как
      // отзыв догонит его по сети доверия: отсюда обновления ему больше не идут.
      doPeerLost(deviceId)

      // Настоящее ядро здесь ещё и рассылает revoke-запись по сети доверия
      // (§2.3). Фейк-ядру рассылать некуда — и притворяться, что оно это
      // сделало, оно не будет.
      return cloneDevice(next)
    },

    // -------------------------------------------------------------------------
    // Статус синхронизации (F10)
    // -------------------------------------------------------------------------

    async getSyncStatus(): Promise<SyncStatus> {
      // За замком: «сколько записей ждёт отправки» и «кто рядом» — сведения о
      // содержимом хранилища и о том, чем пользуется владелец.
      await enter({ requiresUnlock: true })
      return cloneSyncStatus(sync)
    },

    async syncNow(): Promise<SyncStatus> {
      await enter({ requiresUnlock: true })

      // Ручная попытка — та же самая попытка, просто не ждём своей минуты.
      // Настоящее ядро начинает с discovery; фейк-ядро изображает его же.
      setSync({ phase: 'searching', peer_name: null, message: null })
      return cloneSyncStatus(sync)
    },

    // -------------------------------------------------------------------------
    // Конфликты версий (F11, §5.5)
    // -------------------------------------------------------------------------

    async listConflicts(): Promise<RecordConflict[]> {
      await enter({ requiresUnlock: true })

      return conflicts.flatMap((entry) => {
        const record = meta.get(entry.record_id)
        const values = secrets.get(entry.record_id)
        // Запись удалили, пока конфликт ждал: спорить больше не о чем.
        if (record === undefined || record.deleted_at !== null || values === undefined) return []
        return [buildConflict(entry, record, values, THIS_DEVICE_NAME)]
      })
    },

    async resolveConflict(recordId: RecordId, side: ConflictSide): Promise<RecordMeta> {
      await enter({ requiresUnlock: true })

      const entry = conflictFor(recordId)
      const current = liveRecord(recordId)
      const currentSecrets = liveSecrets(recordId)

      const resolvedAt = timestamp()
      // Победившая версия перебивает ОБЕ: иначе при следующем обмене
      // проигравшая вернулась бы как более свежая (§5.2).
      const version = Math.max(current.version, entry.version) + 1

      const nextSecrets = side === 'local' ? currentSecrets : { ...entry.secrets }
      // Секция приехавшей версии могла к этому моменту исчезнуть — тогда
      // запись остаётся там, где лежит: секцию удаляли уже с ней внутри (§4.2).
      const remoteVault = vaults.some((vault) => vault.vault_id === entry.meta.vault_id)
        ? entry.meta.vault_id
        : current.vault_id

      const next: RecordMeta =
        side === 'local'
          ? { ...current, version, updated_at: resolvedAt }
          : {
              ...current,
              vault_id: remoteVault,
              service_name: entry.meta.service_name,
              urls: [...entry.meta.urls],
              login: entry.meta.login,
              account_label: entry.meta.account_label,
              ...secretFlags(nextSecrets),
              version,
              updated_at: resolvedAt,
              password_updated_at:
                nextSecrets.password === currentSecrets.password
                  ? current.password_updated_at
                  : resolvedAt,
            }

      meta.set(recordId, next)
      secrets.set(recordId, nextSecrets)
      // Проигравшая версия НЕ склеивается с победившей и никуда не
      // сохраняется: §5.5 обещает выбор записи целиком.
      conflicts = conflicts.filter((item) => item.record_id !== recordId)
      // Выбор — такое же изменение записи: его ещё надо довезти до второго
      // устройства, иначе там останется своё мнение о том, чья версия верна.
      markPending(next)

      return cloneMeta(next)
    },

    async getConflictSecret(
      recordId: RecordId,
      field: SecretField,
    ): Promise<GetConflictSecretResponse> {
      await enter({ requiresUnlock: true })

      if (field !== 'password' && field !== 'notes' && field !== 'totp_secret') {
        throw new CoreError('VALIDATION', 'У записи нет такого секретного поля.')
      }

      const entry = conflictFor(recordId)
      liveRecord(recordId)
      const currentSecrets = liveSecrets(recordId)

      // Ровно одно поле обеих версий — не вся пара записей целиком.
      return { local: currentSecrets[field], remote: entry.secrets[field] }
    },

    // -------------------------------------------------------------------------
    // Экспорт / импорт / бэкап (F12, §6.2)
    //
    // Фейк-ядро не пишет файлов и не шифрует: настоящие сериализация и крипта
    // живут в Rust. Здесь важно другое — что через границу IPC не проходит ни
    // одного пароля НИ В ОДНУ сторону, и это можно проверить тестом.
    // -------------------------------------------------------------------------

    async exportCsv(masterPasswordInput: string): Promise<ExportFile> {
      await enter({ requiresUnlock: true })
      requireMasterPassword(masterPasswordInput)
      // Уезжает всё живое, включая локальные секции: это файл для переезда,
      // а не синхронизация — «остаётся на устройстве» здесь ни при чём.
      return registerExport('csv', liveRecords().length)
    },

    async exportBackup(masterPasswordInput: string): Promise<ExportFile> {
      await enter({ requiresUnlock: true })
      requireMasterPassword(masterPasswordInput)
      return registerExport('backup', liveRecords().length)
    },

    async deleteExport(path: string): Promise<void> {
      await enter({ requiresUnlock: true })

      if (!exportedFiles.some((file) => file.path === path)) {
        throw new CoreError('NOT_FOUND', 'Этот файл создан не здесь — удалить его отсюда нельзя.')
      }
      exportedFiles = exportedFiles.filter((file) => file.path !== path)
    },

    async restoreBackup(masterPasswordInput: string): Promise<RestoreBackupResult | null> {
      // Восстанавливаются на чистом устройстве, поэтому ни замка, ни хранилища
      // здесь ещё нет.
      await enter({ requiresUnlock: false, requiresInit: false })

      if (initialized) {
        throw new CoreError(
          'ALREADY_INITIALIZED',
          'На этом устройстве уже есть хранилище. Восстановление возможно только на чистом.',
        )
      }
      if (cancelNextPick) {
        cancelNextPick = false
        return null
      }
      // Фейк-ядро изображает файл бэкапа: он закрыт тем же дев-паролем.
      // Настоящее ядро здесь расшифровывает выбранный файл.
      if (masterPasswordInput !== (options.masterPassword ?? MOCK_MASTER_PASSWORD)) {
        throw new CoreError('INVALID_MASTER_PASSWORD', 'Этот бэкап закрыт другим мастер-паролем.')
      }

      masterPassword = masterPasswordInput
      initialized = true
      // Содержимое файла — то же, что лежало бы в хранилище.
      loadSeed()

      const restoredAt = timestamp()
      return {
        file_name: exportLocation('backup', now()).file_name,
        records: liveRecords().length,
        vaults: vaults.length,
        initialized_at: restoredAt,
        unlocked_at: doUnlock(),
      }
    },

    async beginImport(source: ImportSource): Promise<ImportPreview | null> {
      await enter({ requiresUnlock: true })

      if (!IMPORT_SOURCES.includes(source)) {
        throw new CoreError('VALIDATION', 'Неизвестный источник импорта.')
      }
      if (cancelNextPick) {
        cancelNextPick = false
        return null
      }

      const file = createImportFile(source)
      const known = knownPairs()
      const statuses = file.entries.map((entry) =>
        importRowStatus(entry, (host, login) => known.has(`${host} ${login}`)),
      )

      const sessionId = crypto.randomUUID()
      // Разобранные строки остаются ЗДЕСЬ до подтверждения.
      importSessions.set(sessionId, {
        source,
        file_name: file.file_name,
        entries: file.entries,
      })

      const rows: ImportPreviewRow[] = file.entries
        .slice(0, IMPORT_PREVIEW_ROWS)
        .map((entry, index) => ({
          site: entry.site,
          login: entry.login,
          // Пароля в строке предпросмотра нет и быть не может.
          status: statuses[index] ?? 'new',
        }))

      return {
        session_id: sessionId,
        source,
        file_name: file.file_name,
        total_rows: file.entries.length,
        new_count: statuses.filter((status) => status === 'new').length,
        duplicate_count: statuses.filter((status) => status === 'duplicate').length,
        no_password_count: statuses.filter((status) => status === 'no_password').length,
        rows,
        target_vault_name: importVaultName(now()),
      }
    },

    async commitImport(sessionId: ImportSessionId, importOptions: ImportOptions) {
      await enter({ requiresUnlock: true })

      const session = importSessions.get(sessionId)
      if (!session) {
        throw new CoreError('NOT_FOUND', 'Разобранный файл не найден. Выберите его заново.')
      }
      importSessions.delete(sessionId)

      const known = knownPairs()
      const vault = importVault()
      const importedAt = timestamp()

      const taken: MockImportEntry[] = []
      let skipped = 0

      for (const entry of session.entries) {
        const status = importRowStatus(entry, (host, login) => known.has(`${host} ${login}`))
        // Строка без пароля не запись: заводить «пустой» пароль хуже, чем не
        // заводить ничего — он молча притворится рабочим.
        if (status === 'no_password') continue
        if (status === 'duplicate' && importOptions.skip_duplicates) {
          skipped += 1
          continue
        }

        const host = importHost(entry.site)
        const entrySecrets: RecordSecrets = {
          password: entry.password,
          notes: entry.notes,
          totp_secret: entry.totp_secret,
        }
        const record: RecordMeta = {
          record_id: crypto.randomUUID(),
          vault_id: vault.vault_id,
          service_name: serviceNameFromHost(host),
          urls: [host],
          login: entry.login,
          account_label: null,
          ...secretFlags(entrySecrets),
          version: 1,
          created_at: importedAt,
          updated_at: importedAt,
          password_updated_at: importedAt,
          deleted_at: null,
        }

        meta.set(record.record_id, record)
        secrets.set(record.record_id, entrySecrets)
        // Импортированные записи — такие же местные изменения: их ещё надо
        // довезти до второго устройства (F10).
        markPending(record)
        taken.push(entry)
      }

      return {
        imported: taken.length,
        skipped,
        vault: cloneVault(vault),
        // Настоящее ядро здесь правда удаляет разобранный файл. Фейк-ядро его
        // не создавало — но обещание §3.10 отражает честно.
        source_file_deleted: true,
        reused_passwords: importOptions.flag_reused ? countReusedPasswords(taken) : 0,
      } satisfies ImportResult
    },

    async cancelImport(sessionId: ImportSessionId): Promise<void> {
      await enter({ requiresUnlock: true })
      // Идемпотентно, как и `cancelPairing`: по «Отмене» можно нажать дважды.
      importSessions.delete(sessionId)
    },

    on<E extends EventName>(event: E, handler: (payload: EventMap[E]) => void): Unsubscribe {
      const set = handlers.get(event) ?? new Set()
      handlers.set(event, set)
      set.add(handler as (payload: never) => void)
      return () => {
        set.delete(handler as (payload: never) => void)
      }
    },

    control: {
      failNext(code: CoreErrorCode, message = 'Мок-ядро: сымитированная ошибка.') {
        failures.push(new CoreError(code, message))
      },
      setLatency(ms: number) {
        latencyMs = ms
      },
      forceLock(reason) {
        doLock(reason)
      },
      isUnlocked() {
        return unlocked
      },
      isInitialized() {
        return initialized
      },
      peerFound(deviceId) {
        doPeerFound(deviceId)
      },
      peerLost(deviceId) {
        doPeerLost(deviceId)
      },
      startSync(deviceId) {
        doStartSync(deviceId)
      },
      finishSync(failure) {
        doFinishSync(failure)
      },
      raiseConflict(entry) {
        conflicts = [
          ...conflicts.filter((item) => item.record_id !== entry.record_id),
          cloneConflictEntry(entry),
        ]
        const record = meta.get(entry.record_id)
        const values = secrets.get(entry.record_id)
        if (record === undefined || values === undefined) return
        emit('conflict_raised', buildConflict(entry, record, values, THIS_DEVICE_NAME))
      },
      pairedByPeer(name = 'iPhone 14') {
        const pairedAt = timestamp()
        const device: Device = {
          device_id: crypto.randomUUID(),
          name,
          kind: 'mobile',
          is_this_device: false,
          paired_at: pairedAt,
          last_seen_at: pairedAt,
          revoked_at: null,
        }
        devices = [...devices, device]

        const transferred = syncableRecordCount()
        const result: PairingResult = {
          device: { ...device },
          records_transferred: transferred,
          duration_ms: 200 + transferred * 9,
        }
        emit('device_paired', result)
        return result
      },
      cancelNextFilePick() {
        cancelNextPick = true
      },
      enrollPin(next: string) {
        pin = next
        pinAttemptsLeft = MOCK_PIN_ATTEMPTS
      },
      clearPin() {
        forgetPin()
      },
      dispose() {
        stopSimulation()
      },
      reset() {
        cancelNextPick = false
        masterPassword = options.masterPassword ?? MOCK_MASTER_PASSWORD
        initialized = options.initialized !== false
        generatorProfile = cloneProfile(options.generatorProfile ?? DEFAULT_GENERATOR_PROFILE)
        securitySettings = cloneSecuritySettings(
          options.securitySettings ?? initialSecuritySettings(),
        )
        pin = options.pin ?? null
        pinAttemptsLeft = MOCK_PIN_ATTEMPTS
        stopSimulation()
        online.clear()
        loadSeed()
        failures.length = 0
        unlocked = initialized && options.startUnlocked === true
        unlockedAt = unlocked ? timestamp() : null
        if (unlocked) startSimulation()
      },
    },
  }

  return client
}
