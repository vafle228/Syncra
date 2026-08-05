import { CORE_ERROR_CODES, type CoreErrorCode, type CoreErrorPayload } from './contract'

/**
 * Ошибка ядра в форме, удобной UI. Любая ошибка, пришедшая через IPC,
 * нормализуется сюда — и мок, и реальный Tauri-клиент бросают только её.
 */
export class CoreError extends Error {
  readonly code: CoreErrorCode

  constructor(code: CoreErrorCode, message: string) {
    super(message)
    this.name = 'CoreError'
    this.code = code
  }
}

function isCoreErrorCode(value: unknown): value is CoreErrorCode {
  return typeof value === 'string' && (CORE_ERROR_CODES as readonly string[]).includes(value)
}

function isCoreErrorPayload(value: unknown): value is CoreErrorPayload {
  if (typeof value !== 'object' || value === null) return false
  const payload = value as Record<string, unknown>
  return isCoreErrorCode(payload.code) && typeof payload.message === 'string'
}

/**
 * Приводит что угодно, прилетевшее из IPC, к `CoreError`.
 *
 * Умышленно НЕ вкладывает исходное значение в сообщение: неизвестная ошибка
 * может нести отладочные данные ядра, а сообщения об ошибках уходят в UI и логи.
 */
export function toCoreError(raw: unknown): CoreError {
  if (raw instanceof CoreError) return raw
  if (isCoreErrorPayload(raw)) return new CoreError(raw.code, raw.message)
  return new CoreError('INTERNAL', 'Ядро вернуло непредвиденную ошибку.')
}

/** Узкая проверка на конкретный код — для ветвления в UI (напр. на `LOCKED`). */
export function isCoreError(error: unknown, code?: CoreErrorCode): error is CoreError {
  return error instanceof CoreError && (code === undefined || error.code === code)
}
