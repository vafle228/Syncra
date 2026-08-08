// @vitest-environment node
import { describe, expect, it } from 'vitest'

import {
  AUTOLOCK_OPTIONS_MS,
  CLIPBOARD_CLEAR_OPTIONS_MS,
  DEFAULT_SECURITY_SETTINGS,
  SECRET_REVEAL_OPTIONS_MS,
  type CoreErrorCode,
} from '../contract'
import { isCoreError } from '../errors'
import { createMockCoreClient, MOCK_MASTER_PASSWORD } from '../mock'

/**
 * Настройки безопасности (F13, «Настройки → Безопасность»).
 *
 * Проверяется то, ради чего они лежат в ядре: значения переживают замок, вне
 * сетки не принимаются, и патч применяется целиком или никак — половина
 * сохранённых настроек означала бы, что экран показывает неправду.
 */

async function expectCoreError(promise: Promise<unknown>, code: CoreErrorCode): Promise<void> {
  let thrown: unknown = null
  let resolved = false

  try {
    await promise
    resolved = true
  } catch (error) {
    thrown = error
  }

  expect(resolved, `ожидалась ошибка ядра ${code}, но промис зарезолвился`).toBe(false)
  expect(isCoreError(thrown, code), `ожидалась ошибка ядра ${code}, получено: ${thrown}`).toBe(true)
}

describe('getSecuritySettings', () => {
  it('на свежем хранилище отдаёт умолчания', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    await expect(core.getSecuritySettings()).resolves.toEqual(DEFAULT_SECURITY_SETTINGS)
  })

  it('умолчания — средняя ступень каждой тройки', () => {
    // Это свойство держит совместимость: пока ядро не ответило, UI работает по
    // умолчаниям, и они обязаны совпадать с тем, что он делал раньше.
    expect(DEFAULT_SECURITY_SETTINGS.autolock_ms).toBe(AUTOLOCK_OPTIONS_MS[1])
    expect(DEFAULT_SECURITY_SETTINGS.clipboard_clear_ms).toBe(CLIPBOARD_CLEAR_OPTIONS_MS[1])
    expect(DEFAULT_SECURITY_SETTINGS.secret_reveal_ms).toBe(SECRET_REVEAL_OPTIONS_MS[1])
  })

  it('за замком: про защиту закрытого хранилища не рассказываем', async () => {
    const core = createMockCoreClient({ latencyMs: 0 })

    await expectCoreError(core.getSecuritySettings(), 'LOCKED')
    await expectCoreError(core.saveSecuritySettings({ autolock_ms: 60_000 }), 'LOCKED')
  })

  it('принимает начальные настройки от опций мока', async () => {
    const core = createMockCoreClient({
      latencyMs: 0,
      startUnlocked: true,
      securitySettings: {
        autolock_ms: 60_000,
        clipboard_clear_ms: 10_000,
        secret_reveal_ms: 15_000,
      },
    })

    await expect(core.getSecuritySettings()).resolves.toEqual({
      autolock_ms: 60_000,
      clipboard_clear_ms: 10_000,
      secret_reveal_ms: 15_000,
    })
  })
})

describe('saveSecuritySettings', () => {
  it('меняет одну настройку, не трогая остальные', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    const saved = await core.saveSecuritySettings({ clipboard_clear_ms: 60_000 })

    expect(saved).toEqual({ ...DEFAULT_SECURITY_SETTINGS, clipboard_clear_ms: 60_000 })
    await expect(core.getSecuritySettings()).resolves.toEqual(saved)
  })

  it('пустой патч ничего не меняет', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    await expect(core.saveSecuritySettings({})).resolves.toEqual(DEFAULT_SECURITY_SETTINGS)
  })

  it('принимает все ступени из макета', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    for (const autolock_ms of AUTOLOCK_OPTIONS_MS) {
      await expect(core.saveSecuritySettings({ autolock_ms })).resolves.toMatchObject({
        autolock_ms,
      })
    }
    for (const clipboard_clear_ms of CLIPBOARD_CLEAR_OPTIONS_MS) {
      await expect(core.saveSecuritySettings({ clipboard_clear_ms })).resolves.toMatchObject({
        clipboard_clear_ms,
      })
    }
    for (const secret_reveal_ms of SECRET_REVEAL_OPTIONS_MS) {
      await expect(core.saveSecuritySettings({ secret_reveal_ms })).resolves.toMatchObject({
        secret_reveal_ms,
      })
    }
  })

  it('значение вне сетки отклоняет, а не подбирает ближайшее', async () => {
    // Подобрать похожее — значит скрыть расхождение с UI вместо того, чтобы
    // его показать.
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    await expectCoreError(core.saveSecuritySettings({ autolock_ms: 3_000 }), 'VALIDATION')
    await expectCoreError(core.saveSecuritySettings({ clipboard_clear_ms: 0 }), 'VALIDATION')
    await expectCoreError(core.saveSecuritySettings({ secret_reveal_ms: 999 }), 'VALIDATION')

    await expect(core.getSecuritySettings()).resolves.toEqual(DEFAULT_SECURITY_SETTINGS)
  })

  it('патч применяется целиком или никак', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })

    await expectCoreError(
      // Первое значение допустимое, второе — нет.
      core.saveSecuritySettings({ autolock_ms: 60_000, secret_reveal_ms: 7 }),
      'VALIDATION',
    )

    await expect(core.getSecuritySettings()).resolves.toEqual(DEFAULT_SECURITY_SETTINGS)
  })

  it('настройки переживают замок: это часть хранилища, а не состояние экрана', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    await core.saveSecuritySettings({ autolock_ms: 1_800_000 })

    await core.lock()
    await core.unlock(MOCK_MASTER_PASSWORD)

    await expect(core.getSecuritySettings()).resolves.toMatchObject({ autolock_ms: 1_800_000 })
  })

  it('новое хранилище начинает с умолчаний', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })
    await core.initVault('рыжий трамвай у моста')

    await expect(core.getSecuritySettings()).resolves.toEqual(DEFAULT_SECURITY_SETTINGS)
  })

  it('reset возвращает настройки к исходным', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    await core.saveSecuritySettings({ autolock_ms: 60_000 })

    core.control.reset()

    await expect(core.getSecuritySettings()).resolves.toEqual(DEFAULT_SECURITY_SETTINGS)
  })
})
