import type {
  CoreErrorCode,
  EventMap,
  EventName,
  InitVaultResponse,
  IsoDateTime,
  ListRecordsRequest,
  RecordDraft,
  RecordId,
  RecordMeta,
  RecordPatch,
  RecordSecrets,
  UnlockResponse,
  VaultStatus,
} from '../contract'
import { MASTER_PASSWORD_MIN_LENGTH } from '../contract'
import { CoreError } from '../errors'
import type { CoreClient, Unsubscribe } from '../ipc'
import { createSeed, MOCK_MASTER_PASSWORD, MOCK_VAULT_PERSONAL, type MockSeedEntry } from './seed'

export { MOCK_MASTER_PASSWORD, MOCK_VAULT_PERSONAL, MOCK_VAULT_WORK } from './seed'

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
  /** Источник времени — для детерминированных тестов. */
  now?: () => Date
  /** Начинать разблокированным (удобно для UI-тестов, минующих экран входа). */
  startUnlocked?: boolean
  /**
   * Есть ли уже созданное хранилище. `false` — эмуляция первого запуска (F3):
   * сид не загружается, любая команда до `initVault` падает `NOT_INITIALIZED`.
   */
  initialized?: boolean
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
  const failures: CoreError[] = []
  const handlers = new Map<EventName, Set<(payload: never) => void>>()

  let masterPassword = options.masterPassword ?? MOCK_MASTER_PASSWORD
  let initialized = options.initialized !== false
  let unlocked = false
  let unlockedAt: IsoDateTime | null = null

  function timestamp(): IsoDateTime {
    return now().toISOString()
  }

  function loadSeed(): void {
    meta.clear()
    secrets.clear()
    // Свежесозданное хранилище пустое: сид — это «данные, которые уже были».
    if (!initialized) return
    for (const entry of options.seed ?? createSeed()) {
      meta.set(entry.meta.record_id, cloneMeta(entry.meta))
      // У надгробия нет полезной нагрузки — секреты не заводим.
      if (entry.meta.deleted_at === null) {
        secrets.set(entry.meta.record_id, cloneSecrets(entry.secrets))
      }
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
    emit('locked', { locked_at: timestamp(), reason })
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

      const initializedAt = timestamp()
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

      const createdAt = timestamp()
      // ID генерирует ядро, а не UI. Здесь это делает фейк-ядро в роли ядра.
      const record: RecordMeta = {
        record_id: crypto.randomUUID(),
        vault_id: draft.vault_id ?? MOCK_VAULT_PERSONAL,
        service_name: serviceName,
        urls: [...draft.urls],
        login,
        account_label: draft.account_label ?? null,
        version: 1,
        created_at: createdAt,
        updated_at: createdAt,
        password_updated_at: createdAt,
        deleted_at: null,
      }

      meta.set(record.record_id, record)
      secrets.set(record.record_id, {
        password,
        notes: draft.notes ?? null,
        totp_secret: draft.totp_secret ?? null,
      })

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

      const next: RecordMeta = {
        ...current,
        vault_id: patch.vault_id ?? current.vault_id,
        service_name:
          patch.service_name === undefined
            ? current.service_name
            : requireNonEmpty(patch.service_name, 'Сервис'),
        urls: patch.urls === undefined ? current.urls : [...patch.urls],
        login: patch.login === undefined ? current.login : requireNonEmpty(patch.login, 'Логин'),
        account_label:
          patch.account_label === undefined ? current.account_label : patch.account_label,
        version: current.version + 1,
        updated_at: changedAt,
        password_updated_at: passwordChanged ? changedAt : current.password_updated_at,
      }

      meta.set(recordId, next)
      secrets.set(recordId, {
        password: nextPassword,
        notes: patch.notes === undefined ? currentSecrets.notes : patch.notes,
        totp_secret:
          patch.totp_secret === undefined ? currentSecrets.totp_secret : patch.totp_secret,
      })

      return cloneMeta(next)
    },

    async deleteRecord(recordId: RecordId): Promise<RecordMeta> {
      await enter({ requiresUnlock: true })

      const current = liveRecord(recordId)
      const deletedAt = timestamp()
      const tombstone: RecordMeta = {
        ...current,
        version: current.version + 1,
        updated_at: deletedAt,
        deleted_at: deletedAt,
      }

      meta.set(recordId, tombstone)
      // Надгробие не хранит секретов (§5.4).
      secrets.delete(recordId)

      return cloneMeta(tombstone)
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
        loadSeed()
        failures.length = 0
        unlocked = initialized && options.startUnlocked === true
        unlockedAt = unlocked ? timestamp() : null
      },
    },
  }

  return client
}
