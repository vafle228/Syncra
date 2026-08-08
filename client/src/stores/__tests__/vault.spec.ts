import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { setCoreClient } from '@/core/ipc'
import {
  createMockCoreClient,
  MOCK_MASTER_PASSWORD,
  MOCK_PIN_ATTEMPTS,
  type MockCoreClient,
} from '@/core/mock'
import { useVaultStore } from '../useVaultStore'

let core: MockCoreClient

function useVault() {
  return useVaultStore()
}

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0 })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

describe('состояние хранилища', () => {
  it('стартует «не спрашивали» и узнаёт состояние у ядра', async () => {
    const vault = useVault()
    expect(vault.status).toBe('unknown')
    expect(vault.isReady).toBe(false)

    await vault.refresh()

    expect(vault.status).toBe('locked')
    expect(vault.isReady).toBe(true)
  })

  it('распознаёт первый запуск', async () => {
    setCoreClient(createMockCoreClient({ latencyMs: 0, initialized: false }))
    const vault = useVault()

    await vault.refresh()

    expect(vault.status).toBe('uninitialized')
  })

  it('ensureStatus спрашивает ядро один раз на параллельные вызовы', async () => {
    let calls = 0
    const counted = createMockCoreClient({ latencyMs: 0 })
    const original = counted.getVaultStatus.bind(counted)
    counted.getVaultStatus = () => {
      calls += 1
      return original()
    }
    setCoreClient(counted)

    const vault = useVault()
    await Promise.all([vault.ensureStatus(), vault.ensureStatus(), vault.ensureStatus()])

    expect(calls).toBe(1)
    expect(vault.status).toBe('locked')
  })
})

describe('разблокировка', () => {
  it('открывает хранилище правильным паролем', async () => {
    const vault = useVault()

    await vault.unlock(MOCK_MASTER_PASSWORD)

    expect(vault.status).toBe('unlocked')
    expect(vault.isUnlocked).toBe(true)
    expect(vault.unlockedAt).toEqual(expect.any(String))
    expect(vault.error).toBeNull()
  })

  it('на неверном пароле остаётся закрытым и показывает сообщение ядра', async () => {
    const vault = useVault()

    await expect(vault.unlock('не тот пароль')).rejects.toBeTruthy()

    expect(vault.status).toBe('unknown')
    expect(vault.isUnlocked).toBe(false)
    expect(vault.error).toBe('Неверный мастер-пароль.')
  })

  it('ЗАКОН №1: мастер-пароль не оседает в состоянии стора', async () => {
    const vault = useVault()
    const password = 'syncra-dev'

    await vault.unlock(password)

    const snapshot = JSON.stringify(vault.$state)
    expect(snapshot).not.toContain(password)

    // И после неудачной попытки тоже — сообщение об ошибке не эхо-ит ввод.
    await vault.lock()
    await expect(vault.unlock('очень-секретная-фраза')).rejects.toBeTruthy()
    expect(JSON.stringify(vault.$state)).not.toContain('очень-секретная-фраза')
  })

  it('блокирует по команде пользователя', async () => {
    const vault = useVault()
    await vault.unlock(MOCK_MASTER_PASSWORD)

    await vault.lock()

    expect(vault.status).toBe('locked')
    expect(vault.unlockedAt).toBeNull()
    expect(vault.lockReason).toBe('manual')
  })
})

describe('создание хранилища', () => {
  it('инициализирует и сразу оставляет открытым', async () => {
    setCoreClient(createMockCoreClient({ latencyMs: 0, initialized: false }))
    const vault = useVault()
    await vault.refresh()

    await vault.initVault('рыжий трамвай у моста')

    expect(vault.status).toBe('unlocked')
    expect(JSON.stringify(vault.$state)).not.toContain('рыжий трамвай у моста')
  })

  it('показывает сообщение ядра, если пароль не прошёл проверку', async () => {
    setCoreClient(createMockCoreClient({ latencyMs: 0, initialized: false }))
    const vault = useVault()

    await expect(vault.initVault('корот')).rejects.toBeTruthy()

    expect(vault.error).toContain('Мастер-пароль')
    expect(vault.status).toBe('unknown')
  })
})

describe('события ядра', () => {
  it('закрывается по внешнему locked (таймаут бездействия, сон системы)', async () => {
    const vault = useVault()
    await vault.unlock(MOCK_MASTER_PASSWORD)

    core.control.forceLock('timeout')

    expect(vault.status).toBe('locked')
    expect(vault.lockReason).toBe('timeout')
    expect(vault.unlockedAt).toBeNull()
  })

  it('после dispose события больше не двигают состояние', async () => {
    const vault = useVault()
    await vault.unlock(MOCK_MASTER_PASSWORD)

    vault.dispose()
    core.control.forceLock('system')

    expect(vault.status).toBe('unlocked')
  })
})

describe('недоступное ядро', () => {
  it('считает хранилище закрытым, если статус не получен', async () => {
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    const vault = useVault()

    await vault.ensureStatus()

    expect(vault.status).toBe('locked')
    expect(vault.error).toBe('Ядро недоступно.')
  })
})

describe('быстрый вход по PIN (F13)', () => {
  beforeEach(() => {
    setCoreClient(createMockCoreClient({ latencyMs: 0, pin: '1234' }))
  })

  it('узнаёт у ядра, заведён ли PIN на этом устройстве', async () => {
    const vault = useVault()
    expect(vault.pin).toEqual({ enrolled: false, attempts_left: null })

    await vault.refresh()

    expect(vault.pin).toEqual({ enrolled: true, attempts_left: MOCK_PIN_ATTEMPTS })
  })

  it('открывает хранилище верным PIN', async () => {
    const vault = useVault()

    const response = await vault.unlockByPin('1234')

    expect(response.ok).toBe(true)
    expect(vault.status).toBe('unlocked')
    expect(vault.lockReason).toBeNull()
  })

  it('неверный PIN не попадает в error: это опечатка, а не сбой', async () => {
    const vault = useVault()

    const response = await vault.unlockByPin('9999')

    expect(response).toMatchObject({ ok: false, pin_disabled: false })
    expect(vault.error).toBeNull()
    expect(vault.status).toBe('unknown')
    expect(vault.pin.attempts_left).toBe(MOCK_PIN_ATTEMPTS - 1)
  })

  it('исчерпанные попытки убирают быстрый вход из состояния', async () => {
    const vault = useVault()

    for (let attempt = 0; attempt < MOCK_PIN_ATTEMPTS; attempt += 1) {
      await vault.unlockByPin('9999')
    }

    expect(vault.pin).toEqual({ enrolled: false, attempts_left: null })
  })

  it('ЗАКОН №1: набранный PIN не оседает в состоянии стора', async () => {
    const vault = useVault()

    await vault.unlockByPin('9999')
    expect(JSON.stringify(vault.$state)).not.toContain('9999')

    await vault.unlockByPin('1234')
    expect(JSON.stringify(vault.$state)).not.toContain('1234')
  })
})

describe('смена мастер-пароля (F13)', () => {
  const NEXT = 'сосновая шишка на полке'

  beforeEach(() => {
    setCoreClient(createMockCoreClient({ latencyMs: 0, startUnlocked: true, pin: '1234' }))
  })

  it('меняет пароль, не запирая хранилище', async () => {
    const vault = useVault()
    await vault.refresh()

    const response = await vault.changeMasterPassword(MOCK_MASTER_PASSWORD, NEXT)

    expect(response.changed_at).toEqual(expect.any(String))
    expect(vault.status).toBe('unlocked')
  })

  it('сразу убирает быстрый вход: он отпирал ключ от старого пароля', async () => {
    const vault = useVault()
    await vault.refresh()
    expect(vault.pin.enrolled).toBe(true)

    await vault.changeMasterPassword(MOCK_MASTER_PASSWORD, NEXT)

    // Не ждём следующего refresh: иначе экран блокировки успел бы показать
    // клавиатуру, которая больше ничего не отпирает.
    expect(vault.pin).toEqual({ enrolled: false, attempts_left: null })
  })

  it('показывает сообщение ядра, если текущий пароль не подошёл', async () => {
    const vault = useVault()

    await expect(vault.changeMasterPassword('не тот пароль вовсе', NEXT)).rejects.toBeTruthy()

    expect(vault.error).toBe('Неверный мастер-пароль.')
  })

  it('ЗАКОН №1: ни один из двух паролей не оседает в состоянии стора', async () => {
    const vault = useVault()

    await vault.changeMasterPassword(MOCK_MASTER_PASSWORD, NEXT)
    const snapshot = JSON.stringify(vault.$state)
    expect(snapshot).not.toContain(MOCK_MASTER_PASSWORD)
    expect(snapshot).not.toContain(NEXT)

    // И после отказа — сообщение об ошибке не эхо-ит ввод.
    await expect(
      vault.changeMasterPassword('очень-секретная-фраза', 'другая-секретная-фраза'),
    ).rejects.toBeTruthy()
    expect(JSON.stringify(vault.$state)).not.toContain('очень-секретная-фраза')
    expect(JSON.stringify(vault.$state)).not.toContain('другая-секретная-фраза')
  })
})
