import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { resetSecurityPolicy, securityPolicy } from '@/composables/securityPolicy'
import { DEFAULT_SECURITY_SETTINGS } from '@/core/contract'
import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'
import { useSecurityStore } from '../useSecurityStore'

/**
 * Настройки безопасности (F13). Проверяется главное свойство этого стора: он
 * ЕДИНСТВЕННЫЙ писатель в `securityPolicy()`, откуда действующие сроки читают
 * таймеры буфера и авто-скрытия.
 */

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
  resetSecurityPolicy()
})

describe('загрузка', () => {
  it('стартует с умолчаний, а не с null: таймеры работают с первой секунды', () => {
    const security = useSecurityStore()

    expect(security.settings).toEqual(DEFAULT_SECURITY_SETTINGS)
    expect(security.loaded).toBe(false)
  })

  it('забирает настройки у ядра и пробрасывает их в политику', async () => {
    await core.saveSecuritySettings({ autolock_ms: 60_000, secret_reveal_ms: 15_000 })
    const security = useSecurityStore()

    await security.load()

    expect(security.settings).toMatchObject({ autolock_ms: 60_000, secret_reveal_ms: 15_000 })
    expect(securityPolicy().value).toMatchObject({ secret_reveal_ms: 15_000 })
    expect(security.loaded).toBe(true)
  })

  it('ensure() спрашивает ядро один раз за сеанс', async () => {
    const security = useSecurityStore()
    await security.ensure()

    // Ядро успело измениться «снаружи», но повторного запроса не будет.
    await core.saveSecuritySettings({ autolock_ms: 60_000 })
    await security.ensure()

    expect(security.settings.autolock_ms).toBe(DEFAULT_SECURITY_SETTINGS.autolock_ms)
  })

  it('отказ ядра оставляет умолчания рабочими, а не ломает экран', async () => {
    const security = useSecurityStore()
    core.control.failNext('INTERNAL', 'Ядро недоступно.')

    await security.load()

    expect(security.error).toBe('Ядро недоступно.')
    // Таймеры продолжают работать по тем же числам, что и до F13.
    expect(security.settings).toEqual(DEFAULT_SECURITY_SETTINGS)
    expect(securityPolicy().value).toEqual(DEFAULT_SECURITY_SETTINGS)
    expect(security.loaded).toBe(false)
  })
})

describe('сохранение', () => {
  it('меняет настройку и сразу обновляет политику', async () => {
    const security = useSecurityStore()
    await security.ensure()

    await security.save({ clipboard_clear_ms: 10_000 })

    expect(security.settings.clipboard_clear_ms).toBe(10_000)
    expect(securityPolicy().value.clipboard_clear_ms).toBe(10_000)
  })

  it('в состояние идёт ответ ядра, а не отправленный патч', async () => {
    const security = useSecurityStore()

    const saved = await security.save({ autolock_ms: 1_800_000 })

    // Остальные поля пришли от ядра целиком — стор их не додумывает.
    expect(saved).toEqual({ ...DEFAULT_SECURITY_SETTINGS, autolock_ms: 1_800_000 })
    expect(security.settings).toEqual(saved)
  })

  it('отказ ядра не меняет ни состояние, ни политику', async () => {
    const security = useSecurityStore()
    await security.ensure()

    // Значение вне сетки — ядро отвечает VALIDATION.
    await expect(security.save({ autolock_ms: 3_000 })).rejects.toBeTruthy()

    expect(security.settings).toEqual(DEFAULT_SECURITY_SETTINGS)
    expect(securityPolicy().value).toEqual(DEFAULT_SECURITY_SETTINGS)
  })
})

describe('замок', () => {
  it('блокировка возвращает умолчания: настройки — содержимое хранилища', async () => {
    const security = useSecurityStore()
    await security.load()
    await security.save({ secret_reveal_ms: 120_000 })

    core.control.forceLock('timeout')

    expect(security.settings).toEqual(DEFAULT_SECURITY_SETTINGS)
    expect(securityPolicy().value).toEqual(DEFAULT_SECURITY_SETTINGS)
    expect(security.loaded).toBe(false)

    security.dispose()
  })
})

describe('ЗАКОН №1', () => {
  it('в сторе только три числа — ни PIN, ни мастер-пароля', async () => {
    const security = useSecurityStore()
    await security.load()

    expect(Object.keys(security.settings).sort()).toEqual([
      'autolock_ms',
      'clipboard_clear_ms',
      'secret_reveal_ms',
    ])
  })
})
