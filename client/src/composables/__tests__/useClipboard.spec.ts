import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'

import { clearClipboardNow, CLIPBOARD_CLEAR_MS, useClipboard } from '../useClipboard'

/**
 * Обещание продукта, которое здесь проверяется: скопированный пароль исчезает
 * из буфера сам. Если этот тест когда-нибудь начнёт мешать — чинить надо не
 * тест, а причину: обещание напечатано прямо на кнопке.
 */

const writes: string[] = []
let writeText: ReturnType<typeof vi.fn>

/** Запустить composable внутри scope — как в компоненте. */
function run<T>(fn: () => T): { value: T; stop: () => void } {
  const scope = effectScope()
  const value = scope.run(fn) as T
  return { value, stop: () => scope.stop() }
}

beforeEach(() => {
  vi.useFakeTimers()
  writes.length = 0
  writeText = vi.fn(async (text: string) => {
    writes.push(text)
  })
  vi.stubGlobal('navigator', { clipboard: { writeText } })
})

afterEach(async () => {
  await clearClipboardNow()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('useClipboard', () => {
  it('копирует значение и запоминает, какая кнопка сработала', async () => {
    const { value: clipboard, stop } = run(useClipboard)

    const done = await clipboard.copy('password', 'mock-pw', { clearAfterMs: CLIPBOARD_CLEAR_MS })

    expect(done).toBe(true)
    expect(writes).toEqual(['mock-pw'])
    expect(clipboard.copiedKey.value).toBe('password')
    expect(clipboard.secondsLeft.value).toBe(20)

    stop()
  })

  it('очищает буфер через 20 секунд и отсчитывает их в кнопке', async () => {
    const { value: clipboard, stop } = run(useClipboard)
    await clipboard.copy('password', 'mock-pw', { clearAfterMs: CLIPBOARD_CLEAR_MS })

    await vi.advanceTimersByTimeAsync(5000)
    expect(clipboard.secondsLeft.value).toBe(15)
    expect(writes).toEqual(['mock-pw'])

    await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_MS - 5000)

    // Пустая строка — это и есть очистка буфера.
    expect(writes).toEqual(['mock-pw', ''])
    expect(clipboard.copiedKey.value).toBeNull()
    expect(clipboard.secondsLeft.value).toBe(0)

    stop()
  })

  it('не заводит очистку для метаданных: в логине нет секрета', async () => {
    const { value: clipboard, stop } = run(useClipboard)

    await clipboard.copy('login', 'anna@example.com')

    expect(clipboard.secondsLeft.value).toBe(0)
    await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_MS * 2)
    expect(writes).toEqual(['anna@example.com'])

    stop()
  })

  it('дочищает буфер, даже если компонент уже размонтирован', async () => {
    const { value: clipboard, stop } = run(useClipboard)
    await clipboard.copy('password', 'mock-pw', { clearAfterMs: CLIPBOARD_CLEAR_MS })

    // Пользователь закрыл карточку через секунду — обещание про 20 с остаётся.
    stop()
    await vi.advanceTimersByTimeAsync(CLIPBOARD_CLEAR_MS)

    expect(writes).toEqual(['mock-pw', ''])
  })

  it('чистит буфер немедленно по требованию (блокировка хранилища)', async () => {
    const { value: clipboard, stop } = run(useClipboard)
    await clipboard.copy('password', 'mock-pw', { clearAfterMs: CLIPBOARD_CLEAR_MS })

    await clearClipboardNow()

    expect(writes).toEqual(['mock-pw', ''])
    stop()
  })

  it('честно сообщает, что буфер недоступен, и ничего не обещает', async () => {
    vi.stubGlobal('navigator', {})
    const { value: clipboard, stop } = run(useClipboard)

    const done = await clipboard.copy('password', 'mock-pw', { clearAfterMs: CLIPBOARD_CLEAR_MS })

    expect(done).toBe(false)
    expect(clipboard.available.value).toBe(false)
    expect(clipboard.failed.value).toBe(true)
    expect(clipboard.copiedKey.value).toBeNull()

    stop()
  })

  it('не показывает «скопировано», если запись в буфер сорвалась', async () => {
    writeText.mockRejectedValueOnce(new Error('нет разрешения'))
    const { value: clipboard, stop } = run(useClipboard)

    const done = await clipboard.copy('password', 'mock-pw', { clearAfterMs: CLIPBOARD_CLEAR_MS })

    expect(done).toBe(false)
    expect(clipboard.copiedKey.value).toBeNull()
    expect(clipboard.failed.value).toBe(true)

    stop()
  })
})
