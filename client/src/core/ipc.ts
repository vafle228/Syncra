import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

import {
  COMMAND_NAMES,
  EVENT_NAMES,
  type CommandMap,
  type CommandName,
  type ConflictSide,
  type Device,
  type DeviceId,
  type EventMap,
  type EventName,
  type ExportFile,
  type GeneratePasswordsResponse,
  type GeneratorProfile,
  type GetConflictSecretResponse,
  type ImportOptions,
  type ImportPreview,
  type ImportResult,
  type ImportSessionId,
  type ImportSource,
  type InitVaultResponse,
  type ListRecordsRequest,
  type PairingHandshake,
  type PairingOffer,
  type PairingResult,
  type PairingSessionId,
  type RecordConflict,
  type RecordDraft,
  type RecordId,
  type RecordMeta,
  type RecordPatch,
  type RecordSecrets,
  type RestoreBackupResult,
  type SecretField,
  type SyncStatus,
  type UnlockResponse,
  type Vault,
  type VaultColor,
  type VaultId,
  type VaultPatch,
  type VaultStatus,
} from './contract'
import { toCoreError } from './errors'

/** Отписка от события ядра. Идемпотентна. */
export type Unsubscribe = () => void

/**
 * Единственный канал общения UI с ядром.
 *
 * Компоненты НИКОГДА не вызывают Tauri `invoke()` напрямую — только методы
 * этого интерфейса (см. CLAUDE.md, «Граница с ядром»). Мок-ядро и реальный
 * Tauri-клиент взаимозаменяемы за этим интерфейсом.
 */
export interface CoreClient {
  /**
   * Создано ли хранилище и открыто ли оно сейчас. Работает на заблокированном
   * и на неинициализированном хранилище — с этого начинается запуск UI (F3).
   */
  getVaultStatus(): Promise<VaultStatus>

  /**
   * Создать хранилище (§3.9). UI отдаёт только введённый пароль; ключи
   * генерирует ядро. После успеха хранилище уже разблокировано.
   */
  initVault(masterPassword: string): Promise<InitVaultResponse>

  /** Разблокировать хранилище мастер-паролем (§7.3). */
  unlock(masterPassword: string): Promise<UnlockResponse>

  /** Заблокировать хранилище. Ядро сбрасывает расшифрованные данные. */
  lock(): Promise<void>

  /** Метаданные записей. Секретов в ответе нет — по контракту (§4.1). */
  listRecords(request?: ListRecordsRequest): Promise<RecordMeta[]>

  /**
   * Разовый запрос расшифрованных секретов по явному действию пользователя.
   *
   * ЗАКОН №1: результат нельзя класть в стор, localStorage или замыкание
   * дольше показа. Показал / скопировал — отпустил.
   */
  getSecret(recordId: RecordId): Promise<RecordSecrets>

  createRecord(draft: RecordDraft): Promise<RecordMeta>

  updateRecord(recordId: RecordId, patch: RecordPatch): Promise<RecordMeta>

  /** Мягкое удаление: ядро ставит tombstone и возвращает его метаданные (§5.4). */
  deleteRecord(recordId: RecordId): Promise<RecordMeta>

  /** Секции хранилища (F7, §4.2). Имя и цвет — метаданные, не секрет. */
  listVaults(): Promise<Vault[]>

  createVault(name: string, color: VaultColor): Promise<Vault>

  /** Переименовать / перекрасить. Флаг синхронизации сюда не входит. */
  updateVault(vaultId: VaultId, patch: VaultPatch): Promise<Vault>

  /**
   * Включить или выключить синхронизацию секции (§4.2). Выключенная секция
   * остаётся на этом устройстве целиком — вместе со всеми своими записями.
   */
  setVaultSync(vaultId: VaultId, sync: boolean): Promise<Vault>

  /** Назначить секцию по умолчанию. Возвращает весь список: флаг сняли ещё с одной. */
  setDefaultVault(vaultId: VaultId): Promise<Vault[]>

  /**
   * Удалить секцию. Записи не удаляются — ядро переносит их в секцию по
   * умолчанию, поэтому после этого список записей надо перечитать.
   */
  deleteVault(vaultId: VaultId): Promise<Vault[]>

  /** Сохранённые правила генерации (F6, §6.1). Настройки, не секрет. */
  getGeneratorProfile(): Promise<GeneratorProfile>

  /** Сохранить правила «один раз и навсегда». Ядро возвращает их после проверки. */
  saveGeneratorProfile(profile: GeneratorProfile): Promise<GeneratorProfile>

  /**
   * Попросить `count` вариантов пароля (§6.1).
   *
   * Генерация целиком в ядре: случайность берётся у ОС, алфавит и словарь тоже
   * знает только оно. На фронте нет и не должно появиться ни строчки этого кода.
   *
   * ЗАКОН №1: в ответе — пароли открытым текстом. Обращаться как с секретом:
   * показать, дать выбрать, отпустить. Ни в стор, ни в localStorage.
   */
  generatePasswords(count: number, profile?: GeneratorProfile): Promise<GeneratePasswordsResponse>

  /**
   * Код для второго устройства (F8, §2.2): готовая матрица QR и тот же код
   * цифрами-буквами на случай, когда камеры нет.
   *
   * Пейлоад собирает и кодирует ядро — UI получает картинку, а не одноразовый
   * ключ сеанса строкой. Код живёт ограниченное время (`expires_at`).
   */
  getPairingPayload(): Promise<PairingOffer>

  /**
   * Отдать ядру прочитанный код второго устройства.
   *
   * Сопряжение этим ещё не завершается: ядро возвращает слова-отпечаток,
   * которые человек сверяет с экраном второго устройства (§2.2). Пока он не
   * подтвердил, устройство в доверенные НЕ записано.
   */
  submitPairedKey(payload: string): Promise<PairingHandshake>

  /** Слова совпали: записать устройство в доверенные и перенести хранилище. */
  confirmPairing(sessionId: PairingSessionId): Promise<PairingResult>

  /** Слова не совпали или передумали — закрыть сеанс, ключ не запоминать. */
  cancelPairing(sessionId: PairingSessionId): Promise<void>

  /**
   * Доверенные устройства (F9, §2.3). Отозванные тоже здесь — с `revoked_at`:
   * отзыв это факт истории хранилища, а не удаление строки из списка.
   */
  listDevices(): Promise<Device[]>

  /**
   * Отозвать доступ у устройства (§2.3).
   *
   * Отзыв отрезает устройство от БУДУЩИХ синхронизаций. Реплику, которую оно
   * уже скачало, он не стирает и стереть не может — это принятое ограничение
   * (§2.3), и UI обязан сказать об этом до нажатия, а не после.
   */
  revokeDevice(deviceId: DeviceId): Promise<Device>

  /**
   * Что происходит с синхронизацией прямо сейчас (F10).
   *
   * Спрашивается один раз при открытии экрана: дальше состояние приезжает
   * событием `sync_status`. Без этого запроса UI не знал бы состояния до
   * первого события — а его может не быть часами.
   */
  getSyncStatus(): Promise<SyncStatus>

  /**
   * Попробовать обменяться сейчас. Ядро всё равно повторяет попытки само —
   * это только «не жди минуту», а не отдельный режим работы.
   */
  syncNow(): Promise<SyncStatus>

  /** Записи, разошедшиеся на двух устройствах (F11, §5.5). */
  listConflicts(): Promise<RecordConflict[]>

  /**
   * Оставить одну версию целиком (§5.5). Проигравшая версия не склеивается с
   * победившей: пользователь выбирает запись, а не собирает её по полям.
   */
  resolveConflict(recordId: RecordId, side: ConflictSide): Promise<RecordMeta>

  /**
   * Открыть одно секретное поле обеих версий — для сравнения глазами.
   *
   * ЗАКОН №1: правила `getSecret` действуют здесь целиком. Разово, по нажатию,
   * значение не оседает ни в сторе, ни в localStorage.
   */
  getConflictSecret(recordId: RecordId, field: SecretField): Promise<GetConflictSecretResponse>

  /**
   * CSV-экспорт: расшифрованные данные для переезда (F12, §6.2).
   *
   * Файл собирает и пишет ЯДРО — сюда возвращается только след на диске.
   * Содержимое через эту границу не идёт: иначе все пароли хранилища оказались
   * бы одной строкой в JS, чего Закон №1 не допускает даже «на секунду».
   * Мастер-пароль обязателен, даже если хранилище открыто.
   */
  exportCsv(masterPassword: string): Promise<ExportFile>

  /** Зашифрованный бэкап хранилища целиком (§6.2). Крипта — в ядре. */
  exportBackup(masterPassword: string): Promise<ExportFile>

  /**
   * Удалить созданный экспорт («Удалить файл сейчас»). Ядро удаляет только те
   * файлы, которые само создало: произвольный путь сюда передать нельзя.
   */
  deleteExport(path: string): Promise<void>

  /**
   * Восстановить хранилище из бэкапа (§6.3). Только на устройстве без
   * хранилища: поверх живого это было бы слияние двух историй.
   * `null` — человек закрыл окно выбора файла.
   */
  restoreBackup(masterPassword: string): Promise<RestoreBackupResult | null>

  /**
   * Разобрать файл чужого менеджера и показать, что попадёт внутрь (F12).
   *
   * Файл выбирает, читает и разбирает ядро — прямо на этом компьютере.
   * Разобранные строки остаются у него до `commitImport`; сюда приходят
   * метаданные предпросмотра без паролей. `null` — окно выбора закрыли.
   */
  beginImport(source: ImportSource): Promise<ImportPreview | null>

  /** Согласие получено: завести записи из разобранного файла. */
  commitImport(sessionId: ImportSessionId, options: ImportOptions): Promise<ImportResult>

  /** Передумали: ядро забывает разобранные строки вместе с их паролями. */
  cancelImport(sessionId: ImportSessionId): Promise<void>

  /** Подписка на событие ядра. */
  on<E extends EventName>(event: E, handler: (payload: EventMap[E]) => void): Unsubscribe
}

// ---------------------------------------------------------------------------
// Реальный клиент (Tauri IPC)
// ---------------------------------------------------------------------------

async function call<C extends CommandName>(
  command: C,
  request: CommandMap[C]['request'],
): Promise<CommandMap[C]['response']> {
  try {
    return await invoke<CommandMap[C]['response']>(COMMAND_NAMES[command], { request })
  } catch (raw) {
    throw toCoreError(raw)
  }
}

export function createTauriCoreClient(): CoreClient {
  return {
    getVaultStatus: () => call('getVaultStatus', {}),
    initVault: (masterPassword) => call('initVault', { master_password: masterPassword }),
    unlock: (masterPassword) => call('unlock', { master_password: masterPassword }),
    lock: async () => {
      await call('lock', {})
    },
    listRecords: (request = {}) => call('listRecords', request),
    getSecret: (recordId) => call('getSecret', { record_id: recordId }),
    createRecord: (draft) => call('createRecord', { draft }),
    updateRecord: (recordId, patch) => call('updateRecord', { record_id: recordId, patch }),
    deleteRecord: (recordId) => call('deleteRecord', { record_id: recordId }),
    listVaults: () => call('listVaults', {}),
    createVault: (name, color) => call('createVault', { name, color }),
    updateVault: (vaultId, patch) => call('updateVault', { vault_id: vaultId, patch }),
    setVaultSync: (vaultId, sync) => call('setVaultSync', { vault_id: vaultId, sync }),
    setDefaultVault: (vaultId) => call('setDefaultVault', { vault_id: vaultId }),
    deleteVault: (vaultId) => call('deleteVault', { vault_id: vaultId }),
    getGeneratorProfile: () => call('getGeneratorProfile', {}),
    saveGeneratorProfile: (profile) => call('saveGeneratorProfile', { profile }),
    generatePasswords: (count, profile) => call('generatePasswords', { count, profile }),
    getPairingPayload: () => call('getPairingPayload', {}),
    submitPairedKey: (payload) => call('submitPairedKey', { payload }),
    confirmPairing: (sessionId) => call('confirmPairing', { session_id: sessionId }),
    cancelPairing: async (sessionId) => {
      await call('cancelPairing', { session_id: sessionId })
    },
    listDevices: () => call('listDevices', {}),
    revokeDevice: (deviceId) => call('revokeDevice', { device_id: deviceId }),
    getSyncStatus: () => call('getSyncStatus', {}),
    syncNow: () => call('syncNow', {}),
    listConflicts: () => call('listConflicts', {}),
    resolveConflict: (recordId, side) => call('resolveConflict', { record_id: recordId, side }),
    getConflictSecret: (recordId, field) =>
      call('getConflictSecret', { record_id: recordId, field }),
    exportCsv: (masterPassword) => call('exportCsv', { master_password: masterPassword }),
    exportBackup: (masterPassword) => call('exportBackup', { master_password: masterPassword }),
    deleteExport: async (path) => {
      await call('deleteExport', { path })
    },
    restoreBackup: (masterPassword) => call('restoreBackup', { master_password: masterPassword }),
    beginImport: (source) => call('beginImport', { source }),
    commitImport: (sessionId, options) => call('commitImport', { session_id: sessionId, options }),
    cancelImport: async (sessionId) => {
      await call('cancelImport', { session_id: sessionId })
    },

    on: (event, handler) => {
      let cancelled = false
      let unlisten: (() => void) | null = null

      void listen<EventMap[typeof event]>(EVENT_NAMES[event], ({ payload }) => handler(payload))
        .then((fn) => {
          if (cancelled) fn()
          else unlisten = fn
        })
        .catch(() => {
          /* подписка не установилась — событие просто не придёт */
        })

      return () => {
        cancelled = true
        unlisten?.()
        unlisten = null
      }
    },
  }
}

// ---------------------------------------------------------------------------
// Точка переключения мок ↔ реальное ядро
// ---------------------------------------------------------------------------

export type CoreMode = 'mock' | 'tauri'

/**
 * Правило выбора реализации — единственное на весь фронт:
 *  1. `VITE_CORE=mock|tauri` в окружении, если задан явно;
 *  2. иначе прод → реальное ядро, дев и тесты → мок.
 */
export function resolveCoreMode(env: Record<string, unknown> = import.meta.env): CoreMode {
  const explicit = env.VITE_CORE
  if (explicit === 'mock' || explicit === 'tauri') return explicit
  return env.PROD === true ? 'tauri' : 'mock'
}

let client: CoreClient | null = null

/**
 * Создаёт и запоминает клиента ядра. Вызывается один раз из `main.ts` до
 * монтирования приложения. Мок грузится динамически, чтобы фейк-ядро и его
 * сид-данные не попадали в прод-бандл.
 */
export async function initCoreClient(mode: CoreMode = resolveCoreMode()): Promise<CoreClient> {
  if (mode === 'mock') {
    const { createMockCoreClient } = await import('./mock')
    // `VITE_CORE_FRESH=1` — эмуляция первого запуска: хранилища ещё нет,
    // мок-ядро отдаёт NOT_INITIALIZED и UI уходит в онбординг (F3).
    client = createMockCoreClient({
      initialized: import.meta.env.VITE_CORE_FRESH !== '1',
      // В деве фейк-ядро изображает жизнь синхронизации (F10): без второго
      // устройства индикатор иначе навсегда застрял бы в «рядом никого».
      simulateSync: true,
    })
  } else {
    client = createTauriCoreClient()
  }
  return client
}

/** Подменить клиента (тесты, сторибук-подобные сценарии). */
export function setCoreClient(next: CoreClient | null): void {
  client = next
}

/** Доступ к клиенту ядра из компонентов, сторов и composables. */
export function useCore(): CoreClient {
  if (!client) {
    throw new Error('Клиент ядра не инициализирован: вызови initCoreClient() до монтирования.')
  }
  return client
}
