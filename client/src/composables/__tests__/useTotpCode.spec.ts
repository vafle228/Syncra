import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { effectScope, ref } from 'vue'

import type { RecordId } from '@/core/contract'
import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'

import { useTotpCode } from '../useTotpCode'

/**
 * Код подтверждения (F5). Правила те же, что у секретов, плюс одно своё: код
 * живёт окно, и по его истечении на экране должен оказаться следующий код, а
 * не просроченный.
 */

let core: MockCoreClient

/** Запись с подключённым ключом (`seed.ts` — GitHub). */
const GITHUB = '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1003'
/** Запись без ключа — кода у неё нет и быть не может. */
const STEAM = '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1004'

function run(recordId: RecordId | null = GITHUB) {
  const scope = effectScope()
  const id = ref<RecordId | null>(recordId)
  const totp = scope.run(() => useTotpCode(id))!
  return { totp, id, stop: () => scope.stop() }
}

beforeEach(() => {
  vi.useFakeTimers()
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
  vi.useRealTimers()
})

describe('useTotpCode', () => {
  it('до нажатия кода нет', () => {
    const { totp, stop } = run()

    expect(totp.code.value).toBeNull()
    stop()
  })

  it('по нажатию берёт у ядра готовый код и длину окна', async () => {
    const { totp, stop } = run()

    await totp.reveal()

    expect(totp.code.value).toMatch(/^\d{6}$/)
    expect(totp.periodS.value).toBe(30)
    expect(totp.secondsLeft.value).toBeGreaterThan(0)

    stop()
  })

  it('отсчитывает окно и берёт следующий код, когда старый истёк', async () => {
    const { totp, stop } = run()

    await totp.reveal()
    const started = totp.secondsLeft.value

    await vi.advanceTimersByTimeAsync(1000)
    expect(totp.secondsLeft.value).toBe(started - 1)

    // Досидели до конца окна — код обязан обновиться, а не остаться просроченным.
    await vi.advanceTimersByTimeAsync(started * 1000)
    expect(totp.code.value).toMatch(/^\d{6}$/)
    expect(totp.secondsLeft.value).toBeGreaterThan(0)

    stop()
  })

  it('повторное нажатие гасит код и останавливает отсчёт', async () => {
    const { totp, stop } = run()

    await totp.toggle()
    expect(totp.code.value).not.toBeNull()

    await totp.toggle()
    expect(totp.code.value).toBeNull()
    expect(totp.secondsLeft.value).toBe(0)

    // Таймер снят: время идёт, а запросов к ядру больше нет.
    await vi.advanceTimersByTimeAsync(60_000)
    expect(totp.code.value).toBeNull()

    stop()
  })

  it('смена записи гасит чужой код', async () => {
    const { totp, id, stop } = run()

    await totp.reveal()
    expect(totp.code.value).not.toBeNull()

    id.value = STEAM
    await vi.advanceTimersByTimeAsync(0)

    expect(totp.code.value).toBeNull()

    stop()
  })

  it('блокировка хранилища убирает код с экрана', async () => {
    const { totp, stop } = run()

    await totp.reveal()
    expect(totp.code.value).not.toBeNull()

    core.control.forceLock('timeout')
    await vi.advanceTimersByTimeAsync(0)

    expect(totp.code.value).toBeNull()

    stop()
  })

  it('на записи без ключа показывает сообщение ядра, а не пустой код', async () => {
    const { totp, stop } = run(STEAM)

    await totp.reveal()

    expect(totp.code.value).toBeNull()
    expect(totp.error.value).toContain('ключа подтверждения')

    stop()
  })
})
