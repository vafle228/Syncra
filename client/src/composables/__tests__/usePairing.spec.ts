import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { effectScope } from 'vue'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, PAIRING_CODE_TTL_MS, type MockCoreClient } from '@/core/mock'

import { formatManualCode, formatRemaining, usePairingOffer, usePairingScan } from '../usePairing'

/**
 * Сопряжение глазами экрана (F8).
 *
 * ЗАКОН №1 здесь про код сопряжения: тот, кто его прочитал, получает право
 * забрать копию хранилища. Поэтому он живёт в области видимости экрана и
 * исчезает вместе с ним и с замком — это и проверяем.
 */

/** Заведомо чужой код — свой мок отвергает намеренно. */
const FOREIGN_CODE = '4TQ9MB'

let core: MockCoreClient

function runOffer() {
  const scope = effectScope()
  const offer = scope.run(() => usePairingOffer())!
  return { offer, stop: () => scope.stop() }
}

function runScan() {
  const scope = effectScope()
  const scan = scope.run(() => usePairingScan())!
  return { scan, stop: () => scope.stop() }
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-08-08T10:00:00.000Z'))
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
  vi.useRealTimers()
})

describe('сторона, которая показывает код', () => {
  it('берёт код у ядра и считает, сколько ему осталось', async () => {
    const { offer, stop } = runOffer()

    await offer.request()

    expect(offer.offer.value?.manual_code).toMatch(/^[2-9A-HJ-NP-Z]{6}$/)
    expect(offer.remainingLabel.value).toBe('03:00')
    expect(offer.isExpired.value).toBe(false)

    stop()
  })

  it('тикает обратный отсчёт и помечает код истёкшим', async () => {
    const { offer, stop } = runOffer()
    await offer.request()

    await vi.advanceTimersByTimeAsync(19_000)
    expect(offer.remainingLabel.value).toBe('02:41')

    await vi.advanceTimersByTimeAsync(PAIRING_CODE_TTL_MS)
    // Код с экрана не пропадает: человек должен понять, почему второе
    // устройство его не берёт.
    expect(offer.offer.value).not.toBeNull()
    expect(offer.isExpired.value).toBe(true)
    expect(offer.remainingLabel.value).toBe('00:00')

    stop()
  })

  it('каждый запрос — новый код', async () => {
    const { offer, stop } = runOffer()

    await offer.request()
    const first = offer.offer.value?.manual_code
    await offer.request()

    expect(offer.offer.value?.manual_code).not.toBe(first)

    stop()
  })

  it('замок стирает код с экрана', async () => {
    const { offer, stop } = runOffer()
    await offer.request()

    core.control.forceLock('timeout')

    expect(offer.offer.value).toBeNull()

    stop()
  })

  it('уход с экрана стирает код', async () => {
    const { offer, stop } = runOffer()
    await offer.request()

    stop()

    expect(offer.offer.value).toBeNull()
  })

  it('отказ ядра показывает сообщением, а не пустым экраном', async () => {
    const { offer, stop } = runOffer()
    core.control.failNext('INTERNAL', 'Ядро не смогло собрать код.')

    expect(await offer.request()).toBe(false)
    expect(offer.error.value).toBe('Ядро не смогло собрать код.')
    expect(offer.offer.value).toBeNull()

    stop()
  })
})

describe('сторона, которая читает код', () => {
  it('получает слова для сверки, но ничего ещё не сопрягает', async () => {
    const { scan, stop } = runScan()

    expect(await scan.submit(FOREIGN_CODE)).toBe(true)
    expect(scan.handshake.value?.fingerprint_words).toHaveLength(4)
    expect(scan.result.value).toBeNull()

    stop()
  })

  it('ЗАКОН №1: прочитанный код не оседает в состоянии', async () => {
    const { scan, stop } = runScan()

    await scan.submit(FOREIGN_CODE)

    const state = JSON.stringify({
      handshake: scan.handshake.value,
      result: scan.result.value,
      error: scan.error.value,
    })
    expect(state).not.toContain(FOREIGN_CODE)

    stop()
  })

  it('подтверждение записывает устройство и закрывает сеанс', async () => {
    const { scan, stop } = runScan()
    await scan.submit(FOREIGN_CODE)

    expect(await scan.confirm()).toBe(true)
    expect(scan.result.value?.device.is_this_device).toBe(false)
    expect(scan.handshake.value).toBeNull()

    stop()
  })

  it('без прочитанного кода подтверждать нечего', async () => {
    const { scan, stop } = runScan()

    expect(await scan.confirm()).toBe(false)

    stop()
  })

  it('отмена закрывает сеанс и в ядре тоже', async () => {
    const { scan, stop } = runScan()
    await scan.submit(FOREIGN_CODE)
    const session = scan.handshake.value?.session_id as string

    await scan.cancel()

    expect(scan.handshake.value).toBeNull()
    // Ядро о сеансе больше не знает: подтвердить его задним числом нельзя.
    await expect(core.confirmPairing(session)).rejects.toThrow()

    stop()
  })

  it('мусор вместо кода показывает сообщением ядра', async () => {
    const { scan, stop } = runScan()

    expect(await scan.submit('привет')).toBe(false)
    expect(scan.error.value).toContain('не похоже на код Syncra')
    expect(scan.isExpired.value).toBe(false)
    expect(scan.handshake.value).toBeNull()

    stop()
  })

  it('устаревший код отличает от нечитаемого — экрану есть что предложить', async () => {
    const { scan, stop } = runScan()
    core.control.failNext('PAIRING_EXPIRED', 'Этот код больше не действует.')

    expect(await scan.submit(FOREIGN_CODE)).toBe(false)
    expect(scan.isExpired.value).toBe(true)

    stop()
  })

  it('пустую строку в ядро не отправляет', async () => {
    const { scan, stop } = runScan()

    expect(await scan.submit('   ')).toBe(false)
    expect(scan.error.value).toBeNull()

    stop()
  })

  it('замок стирает начатое сопряжение', async () => {
    const { scan, stop } = runScan()
    await scan.submit(FOREIGN_CODE)

    core.control.forceLock('system')

    expect(scan.handshake.value).toBeNull()
    expect(scan.result.value).toBeNull()

    stop()
  })
})

describe('форматирование', () => {
  it('обратный отсчёт — как на макете', () => {
    expect(formatRemaining(161_000)).toBe('02:41')
    expect(formatRemaining(0)).toBe('00:00')
    // Отрицательного времени не бывает даже при рассинхроне часов.
    expect(formatRemaining(-5_000)).toBe('00:00')
  })

  it('код разбивается на две половины — его диктуют вслух', () => {
    expect(formatManualCode('4TQ9MB')).toBe('4TQ · 9MB')
  })
})
