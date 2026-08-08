import type {
  CoreErrorCode,
  Device,
  DeviceId,
  EventMap,
  EventName,
  GeneratePasswordsResponse,
  GeneratorProfile,
  InitVaultResponse,
  IsoDateTime,
  ListRecordsRequest,
  PairingHandshake,
  PairingOffer,
  PairingResult,
  PairingSessionId,
  RecordDraft,
  RecordId,
  RecordMeta,
  RecordPatch,
  RecordSecrets,
  UnlockResponse,
  Vault,
  VaultColor,
  VaultId,
  VaultPatch,
  VaultStatus,
} from '../contract'
import { MASTER_PASSWORD_MIN_LENGTH, VAULT_COLORS, VAULT_NAME_MAX_LENGTH } from '../contract'
import { CoreError } from '../errors'
import type { CoreClient, Unsubscribe } from '../ipc'
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
  unreadableCode,
} from './pairing'
import {
  createDeviceSeed,
  createInitialVault,
  createSeed,
  createVaultSeed,
  MOCK_MASTER_PASSWORD,
  type MockSeedEntry,
} from './seed'

export {
  createVaultSeed,
  MOCK_DEVICE_LAPTOP,
  MOCK_DEVICE_PHONE,
  MOCK_MASTER_PASSWORD,
  MOCK_VAULT_PERSONAL,
  MOCK_VAULT_WORK,
} from './seed'
export { DEFAULT_GENERATOR_PROFILE } from './generator'
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

/**
 * Выводит метаданные-флаги из самих секретов. Единственный источник правды о
 * том, заполнены ли `notes` / `totp_secret`: ядро тоже считает их у себя, а не
 * принимает от UI.
 */
function secretFlags(secrets: RecordSecrets): Pick<RecordMeta, 'has_notes' | 'has_totp'> {
  return {
    has_notes: (secrets.notes ?? '').trim() !== '',
    has_totp: (secrets.totp_secret ?? '').trim() !== '',
  }
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

  let masterPassword = options.masterPassword ?? MOCK_MASTER_PASSWORD
  let initialized = options.initialized !== false
  let unlocked = false
  let unlockedAt: IsoDateTime | null = null
  /** Профиль генератора живёт в хранилище рядом с записями (F6). */
  let generatorProfile: GeneratorProfile = cloneProfile(
    options.generatorProfile ?? DEFAULT_GENERATOR_PROFILE,
  )

  function timestamp(): IsoDateTime {
    return now().toISOString()
  }

  function loadSeed(): void {
    meta.clear()
    secrets.clear()
    vaults = []
    devices = []
    forgetPairing()
    // Свежесозданное хранилище пустое: сид — это «данные, которые уже были».
    if (!initialized) return

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
    emit('locked', { locked_at: timestamp(), reason })
  }

  /** Забыть код и все начатые сеансы сопряжения. */
  function forgetPairing(): void {
    offer = null
    retiredCodes.clear()
    handshakes.clear()
  }

  function expired(at: IsoDateTime): boolean {
    return Date.parse(at) <= now().getTime()
  }

  loadSeed()
  if (options.startUnlocked && initialized) {
    unlocked = true
    unlockedAt = timestamp()
  }

  const client: MockCoreClient = {
    async getVaultStatus(): Promise<VaultStatus> {
      // Статус доступен всегда: с него начинается запуск UI.
      await enter({ requiresUnlock: false, requiresInit: false })
      return { initialized, unlocked, unlocked_at: unlockedAt }
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

      return { unlocked_at: doUnlock() }
    },

    async lock(): Promise<void> {
      await enter({ requiresUnlock: false })
      doLock('manual')
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
      const syncing = new Set(vaults.filter((vault) => vault.sync).map((vault) => vault.vault_id))
      const transferred = [...meta.values()].filter(
        (record) => record.deleted_at === null && syncing.has(record.vault_id),
      ).length

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

      // Настоящее ядро здесь ещё и рассылает revoke-запись по сети доверия
      // (§2.3). Фейк-ядру рассылать некуда — и притворяться, что оно это
      // сделало, оно не будет.
      return cloneDevice(next)
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
      reset() {
        masterPassword = options.masterPassword ?? MOCK_MASTER_PASSWORD
        initialized = options.initialized !== false
        generatorProfile = cloneProfile(options.generatorProfile ?? DEFAULT_GENERATOR_PROFILE)
        loadSeed()
        failures.length = 0
        unlocked = initialized && options.startUnlocked === true
        unlockedAt = unlocked ? timestamp() : null
      },
    },
  }

  return client
}
