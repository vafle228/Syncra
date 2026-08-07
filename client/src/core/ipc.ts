import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

import {
  COMMAND_NAMES,
  EVENT_NAMES,
  type CommandMap,
  type CommandName,
  type EventMap,
  type EventName,
  type GeneratePasswordsResponse,
  type GeneratorProfile,
  type InitVaultResponse,
  type ListRecordsRequest,
  type RecordDraft,
  type RecordId,
  type RecordMeta,
  type RecordPatch,
  type RecordSecrets,
  type UnlockResponse,
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
    getGeneratorProfile: () => call('getGeneratorProfile', {}),
    saveGeneratorProfile: (profile) => call('saveGeneratorProfile', { profile }),
    generatePasswords: (count, profile) => call('generatePasswords', { count, profile }),

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
    client = createMockCoreClient({ initialized: import.meta.env.VITE_CORE_FRESH !== '1' })
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
