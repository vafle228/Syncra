import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, DEFAULT_GENERATOR_PROFILE, type MockCoreClient } from '@/core/mock'

import { useGeneratorStore } from '../useGeneratorStore'

/**
 * Стор профиля генератора (F6). В нём лежат ПРАВИЛА и только они: поля под
 * сгенерированный пароль здесь нет по устройству, и это проверяется явно.
 */

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

describe('useGeneratorStore', () => {
  it('забирает профиль у ядра', async () => {
    const store = useGeneratorStore()

    expect(store.profile).toBeNull()
    expect(store.loaded).toBe(false)

    await store.load()

    expect(store.profile).toEqual(DEFAULT_GENERATOR_PROFILE)
    expect(store.loaded).toBe(true)
    expect(store.error).toBeNull()

    store.dispose()
  })

  it('ensure() спрашивает ядро один раз', async () => {
    const store = useGeneratorStore()
    const spy = vi.spyOn(core, 'getGeneratorProfile')

    await store.ensure()
    await store.ensure()
    await store.ensure()

    expect(spy).toHaveBeenCalledTimes(1)

    spy.mockRestore()
    store.dispose()
  })

  it('показывает сообщение ядра, если правила не пришли', async () => {
    const store = useGeneratorStore()
    core.control.failNext('INTERNAL', 'Хранилище занято.')

    await store.load()

    expect(store.error).toBe('Хранилище занято.')
    expect(store.profile).toBeNull()
    expect(store.loaded).toBe(false)

    store.dispose()
  })

  it('сохраняет правила и держит у себя то, что вернуло ядро', async () => {
    const store = useGeneratorStore()
    await store.ensure()

    await store.save({ ...DEFAULT_GENERATOR_PROFILE, mode: 'words', words: 6 })

    expect(store.profile).toMatchObject({ mode: 'words', words: 6 })
    await expect(core.getGeneratorProfile()).resolves.toMatchObject({ mode: 'words', words: 6 })

    store.dispose()
  })

  it('ошибку сохранения отдаёт наверх, а не подменяет ею настройки', async () => {
    const store = useGeneratorStore()
    await store.ensure()
    core.control.failNext('VALIDATION', 'Длина вне допустимых границ.')

    await expect(store.save({ ...DEFAULT_GENERATOR_PROFILE, length: 4 })).rejects.toThrow(
      'Длина вне допустимых границ.',
    )

    // Настройки на экране остались прежними — их не смахнуло сообщением.
    expect(store.profile).toEqual(DEFAULT_GENERATOR_PROFILE)
    expect(store.error).toBeNull()

    store.dispose()
  })

  it('блокировка хранилища очищает профиль', async () => {
    const store = useGeneratorStore()
    await store.ensure()

    core.control.forceLock('timeout')
    await nextTick()

    expect(store.profile).toBeNull()
    expect(store.loaded).toBe(false)

    store.dispose()
  })

  it('ЗАКОН №1: в состоянии нет ни одного сгенерированного пароля', async () => {
    const store = useGeneratorStore()
    await store.ensure()

    const { passwords } = await core.generatePasswords(5)

    const snapshot = JSON.stringify(store.$state)
    for (const password of passwords) expect(snapshot).not.toContain(password)
    // И поля под них тоже нет — не «сейчас пусто», а «класть некуда».
    expect(Object.keys(store.$state)).toEqual(['profile', 'loading', 'saving', 'error', 'loaded'])

    store.dispose()
  })
})
