import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { effectScope, nextTick } from 'vue'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, DEFAULT_GENERATOR_PROFILE, type MockCoreClient } from '@/core/mock'
import { useGeneratorStore } from '@/stores/useGeneratorStore'

import {
  sameProfile,
  useDebounced,
  useGeneratorProfileDraft,
  usePasswordGenerator,
} from '../usePasswordGenerator'

/**
 * ЗАКОН №1 для генератора (F6): свежий пароль — ещё ничей секрет, но живёт он
 * по тем же правилам, что и чужой: в области видимости компонента, до закрытия
 * панели или до блокировки хранилища.
 */

let core: MockCoreClient

function run<T>(factory: () => T): { value: T; stop: () => void } {
  const scope = effectScope()
  return { value: scope.run(factory) as T, stop: () => scope.stop() }
}

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

describe('usePasswordGenerator', () => {
  it('просит у ядра варианты и показывает их вместе с оценкой ядра', async () => {
    const { value: generator, stop } = run(() => usePasswordGenerator())

    expect(await generator.generate(5)).toBe(true)

    expect(generator.variants.value).toHaveLength(5)
    expect(generator.hasVariants.value).toBe(true)
    expect(generator.entropyBits.value).toBeGreaterThan(0)
    expect(generator.pickedIndex.value).toBeNull()

    stop()
  })

  it('ничего не выбирает само — выбор делает человек', async () => {
    const { value: generator, stop } = run(() => usePasswordGenerator())
    await generator.generate(3)

    expect(generator.pickedIndex.value).toBeNull()

    const chosen = generator.pick(1)
    expect(chosen).toBe(generator.variants.value[1])
    expect(generator.pickedIndex.value).toBe(1)

    stop()
  })

  it('не отмечает несуществующий вариант', async () => {
    const { value: generator, stop } = run(() => usePasswordGenerator())
    await generator.generate(3)

    expect(generator.pick(9)).toBeNull()
    expect(generator.pickedIndex.value).toBeNull()

    stop()
  })

  it('снимает отметку, когда список вариантов сменился', async () => {
    const { value: generator, stop } = run(() => usePasswordGenerator())
    await generator.generate(3)
    generator.pick(2)

    await generator.generate(3)

    // Иначе отметка указывала бы на строку, которой пользователь не выбирал.
    expect(generator.pickedIndex.value).toBeNull()

    stop()
  })

  it('показывает сообщение ядра и не оставляет вариантов при ошибке', async () => {
    const { value: generator, stop } = run(() => usePasswordGenerator())
    await generator.generate(3)
    core.control.failNext('INTERNAL', 'Ядро занято.')

    expect(await generator.generate(3)).toBe(false)

    expect(generator.error.value).toBe('Ядро занято.')
    expect(generator.variants.value).toEqual([])
    expect(generator.entropyBits.value).toBe(0)

    stop()
  })
})

describe('ЗАКОН №1', () => {
  it('сгенерированные пароли не попадают в Pinia', async () => {
    const store = useGeneratorStore()
    await store.ensure()

    const { value: generator, stop } = run(() => usePasswordGenerator())
    await generator.generate(5)

    const shown = generator.variants.value
    expect(shown).toHaveLength(5)

    // Пароли на экране — а в состоянии приложения их нет.
    const snapshot = JSON.stringify(store.$state)
    for (const password of shown) expect(snapshot).not.toContain(password)

    stop()
  })

  it('блокировка хранилища стирает варианты с экрана', async () => {
    const { value: generator, stop } = run(() => usePasswordGenerator())
    await generator.generate(5)
    generator.pick(0)

    core.control.forceLock('timeout')
    await nextTick()

    expect(generator.variants.value).toEqual([])
    expect(generator.pickedIndex.value).toBeNull()
    expect(generator.entropyBits.value).toBe(0)

    stop()
  })

  it('закрытие панели забывает варианты', async () => {
    const { value: generator, stop } = run(() => usePasswordGenerator())
    await generator.generate(5)

    generator.forget()

    expect(generator.variants.value).toEqual([])
    expect(generator.hasVariants.value).toBe(false)

    stop()
  })

  it('размонтирование забывает варианты', async () => {
    const { value: generator, stop } = run(() => usePasswordGenerator())
    await generator.generate(5)

    stop()

    expect(generator.variants.value).toEqual([])
  })
})

describe('черновик профиля', () => {
  it('снимается с сохранённого профиля и сначала не считается изменённым', async () => {
    const { value: draft, stop } = run(() => useGeneratorProfileDraft())

    await draft.ensure()

    expect(draft.draft.value).toEqual(DEFAULT_GENERATOR_PROFILE)
    expect(draft.dirty.value).toBe(false)

    stop()
  })

  it('правка не уходит в ядро, пока не нажали «Сохранить» (§6.1)', async () => {
    const { value: draft, stop } = run(() => useGeneratorProfileDraft())
    await draft.ensure()

    draft.set({ ...DEFAULT_GENERATOR_PROFILE, length: 32 })

    expect(draft.dirty.value).toBe(true)
    // Ядро всё ещё держит прежние правила.
    await expect(core.getGeneratorProfile()).resolves.toEqual(DEFAULT_GENERATOR_PROFILE)

    expect(await draft.save()).toBe(true)
    expect(draft.dirty.value).toBe(false)
    await expect(core.getGeneratorProfile()).resolves.toMatchObject({ length: 32 })

    stop()
  })

  it('показывает сообщение ядра, если сохранить не удалось', async () => {
    const { value: draft, stop } = run(() => useGeneratorProfileDraft())
    await draft.ensure()
    draft.set({ ...DEFAULT_GENERATOR_PROFILE, length: 32 })
    core.control.failNext('INTERNAL', 'Хранилище занято.')

    expect(await draft.save()).toBe(false)

    expect(draft.saveError.value).toBe('Хранилище занято.')
    // Правка не потерялась: пользователю есть что сохранить повторно.
    expect(draft.dirty.value).toBe(true)

    stop()
  })

  it('sameProfile сравнивает все поля, а не ссылку', () => {
    const base = DEFAULT_GENERATOR_PROFILE

    expect(sameProfile(base, { ...base })).toBe(true)
    expect(sameProfile(base, { ...base, append_number: !base.append_number })).toBe(false)
    expect(sameProfile(base, { ...base, separator: '.' })).toBe(false)
  })
})

describe('useDebounced', () => {
  it('схлопывает череду вызовов в один', async () => {
    vi.useFakeTimers()
    const fn = vi.fn()
    const { value: call, stop } = run(() => useDebounced(fn, 200))

    call()
    call()
    call()
    expect(fn).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(200)
    expect(fn).toHaveBeenCalledTimes(1)

    stop()
    vi.useRealTimers()
  })

  it('не срабатывает после размонтирования', async () => {
    vi.useFakeTimers()
    const fn = vi.fn()
    const { value: call, stop } = run(() => useDebounced(fn, 200))

    call()
    stop()
    await vi.advanceTimersByTimeAsync(200)

    expect(fn).not.toHaveBeenCalled()
    vi.useRealTimers()
  })
})
