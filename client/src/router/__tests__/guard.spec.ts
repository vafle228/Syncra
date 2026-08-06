import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory } from 'vue-router'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_MASTER_PASSWORD } from '@/core/mock'
import { createAppRouter, routeForStatus } from '../index'

beforeEach(() => {
  setActivePinia(createPinia())
})

afterEach(() => {
  setCoreClient(null)
})

describe('routeForStatus', () => {
  it('без хранилища пускает только в онбординг', () => {
    expect(routeForStatus('uninitialized', 'home')).toEqual({ name: 'setup' })
    expect(routeForStatus('uninitialized', 'unlock')).toEqual({ name: 'setup' })
    expect(routeForStatus('uninitialized', 'setup')).toBe(true)
  })

  it('на закрытом хранилище пускает только на экран входа', () => {
    expect(routeForStatus('locked', 'home')).toEqual({ name: 'unlock' })
    expect(routeForStatus('locked', 'setup')).toEqual({ name: 'unlock' })
    expect(routeForStatus('locked', 'unlock')).toBe(true)
  })

  it('неизвестное состояние трактует как закрытое', () => {
    expect(routeForStatus('unknown', 'home')).toEqual({ name: 'unlock' })
  })

  it('на открытом хранилище не держит на экранах входа', () => {
    expect(routeForStatus('unlocked', 'home')).toBe(true)
    expect(routeForStatus('unlocked', 'unlock')).toEqual({ name: 'home' })
    expect(routeForStatus('unlocked', 'setup')).toEqual({ name: 'home' })
  })
})

describe('навигационный хранитель', () => {
  it('уводит на экран входа при попытке открыть главный экран', async () => {
    setCoreClient(createMockCoreClient({ latencyMs: 0 }))
    const router = createAppRouter(createMemoryHistory())

    await router.push('/')

    expect(router.currentRoute.value.name).toBe('unlock')
  })

  it('уводит в онбординг, если хранилища ещё нет', async () => {
    setCoreClient(createMockCoreClient({ latencyMs: 0, initialized: false }))
    const router = createAppRouter(createMemoryHistory())

    await router.push('/')

    expect(router.currentRoute.value.name).toBe('setup')
  })

  it('пускает на главный экран после разблокировки', async () => {
    const core = createMockCoreClient({ latencyMs: 0 })
    setCoreClient(core)
    await core.unlock(MOCK_MASTER_PASSWORD)

    const router = createAppRouter(createMemoryHistory())
    await router.push('/')

    expect(router.currentRoute.value.name).toBe('home')
  })

  it('не даёт вернуться на главный экран после блокировки', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    setCoreClient(core)

    const router = createAppRouter(createMemoryHistory())
    await router.push('/')
    expect(router.currentRoute.value.name).toBe('home')

    // Замок защёлкнулся сам — таймаут бездействия. Любой следующий переход
    // должен упереться в экран входа, а не пустить дальше по приложению.
    core.control.forceLock('timeout')
    await router.push('/setup')

    expect(router.currentRoute.value.name).toBe('unlock')
  })
})
