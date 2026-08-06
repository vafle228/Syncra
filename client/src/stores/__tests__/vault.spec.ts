import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_MASTER_PASSWORD, type MockCoreClient } from '@/core/mock'
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
