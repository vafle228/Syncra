import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { effectScope, ref } from 'vue'

import type { RecordId } from '@/core/contract'
import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'
import { useRecordsStore } from '@/stores/useRecordsStore'

import { clearClipboardNow } from '../useClipboard'
import { SECRET_AUTO_HIDE_MS, useRecordSecrets } from '../useRecordSecrets'

/**
 * ЗАКОН №1 в исполнении: секрет приходит разово, живёт ровно пока показан и
 * не остаётся ни в Pinia, ни после блокировки хранилища.
 */

let core: MockCoreClient
const writes: string[] = []

const GITHUB = '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1003'

function run(recordId: RecordId | null = GITHUB) {
  const scope = effectScope()
  const id = ref<RecordId | null>(recordId)
  const secrets = scope.run(() => useRecordSecrets(id))!
  return { secrets, id, stop: () => scope.stop() }
}

beforeEach(() => {
  vi.useFakeTimers()
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)

  writes.length = 0
  vi.stubGlobal('navigator', {
    clipboard: {
      writeText: vi.fn(async (text: string) => {
        writes.push(text)
      }),
    },
  })
})

afterEach(async () => {
  await clearClipboardNow()
  setCoreClient(null)
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('reveal', () => {
  it('запрашивает секрет у ядра и показывает только запрошенное поле', async () => {
    const { secrets, stop } = run()

    await secrets.reveal('password')

    expect(secrets.shown.password).toBe('mock-github-pw')
    // Ответ ядра содержал и заметки, и ключ TOTP — но их не просили.
    expect(secrets.shown.notes).toBeNull()
    expect(secrets.shown.totp_secret).toBeNull()

    stop()
  })

  it('закрывает поле само через 30 секунд и отсчитывает их на экране', async () => {
    const { secrets, stop } = run()
    await secrets.reveal('password')

    expect(secrets.hideIn.password).toBe(30)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(secrets.hideIn.password).toBe(20)
    expect(secrets.shown.password).toBe('mock-github-pw')

    await vi.advanceTimersByTimeAsync(SECRET_AUTO_HIDE_MS - 10_000)

    expect(secrets.shown.password).toBeNull()
    expect(secrets.hideIn.password).toBe(0)

    stop()
  })

  it('закрывает поле по повторному нажатию', async () => {
    const { secrets, stop } = run()

    await secrets.toggle('password')
    expect(secrets.shown.password).toBe('mock-github-pw')

    await secrets.toggle('password')
    expect(secrets.shown.password).toBeNull()

    stop()
  })

  it('переключение на другую запись немедленно закрывает открытое', async () => {
    const { secrets, id, stop } = run()
    await secrets.reveal('password')

    id.value = '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1004'
    await vi.advanceTimersByTimeAsync(0)

    expect(secrets.shown.password).toBeNull()

    stop()
  })

  it('показывает сообщение ядра и ничего не открывает при ошибке', async () => {
    const { secrets, stop } = run()
    core.control.failNext('NOT_FOUND', 'Запись не найдена.')

    await secrets.reveal('password')

    expect(secrets.error.value).toBe('Запись не найдена.')
    expect(secrets.shown.password).toBeNull()

    stop()
  })
})

describe('копирование', () => {
  it('кладёт пароль в буфер, не показывая его на экране', async () => {
    const { secrets, stop } = run()

    const done = await secrets.copy('password')

    expect(done).toBe(true)
    expect(writes).toEqual(['mock-github-pw'])
    // Главное: копирование не потребовало открывать поле.
    expect(secrets.shown.password).toBeNull()
    expect(secrets.clipboard.copiedKey.value).toBe('password')

    stop()
  })

  it('не копирует пустое поле', async () => {
    // У записи Steam нет ни заметок, ни ключа TOTP.
    const { secrets, stop } = run('6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1004')

    expect(await secrets.copy('notes')).toBe(false)
    expect(writes).toEqual([])

    stop()
  })
})

describe('ЗАКОН №1', () => {
  it('открытый секрет не попадает в Pinia', async () => {
    const list = useRecordsStore()
    await list.load()
    list.select(GITHUB)

    const { secrets, stop } = run()
    await secrets.reveal('password')
    await secrets.reveal('notes')

    // Секрет на экране — а в состоянии приложения его нет.
    expect(secrets.shown.password).toBe('mock-github-pw')
    const snapshot = JSON.stringify(list.$state)
    expect(snapshot).not.toContain('mock-github-pw')
    expect(snapshot).not.toContain('Recovery codes')

    stop()
  })

  it('блокировка хранилища закрывает секреты и чистит буфер', async () => {
    const { secrets, stop } = run()
    await secrets.reveal('password')
    await secrets.copy('password')
    expect(writes).toEqual(['mock-github-pw'])

    // Ушёл от компьютера — сработал таймаут ядра.
    core.control.forceLock('timeout')
    await vi.advanceTimersByTimeAsync(0)

    expect(secrets.shown.password).toBeNull()
    expect(secrets.clipboard.copiedKey.value).toBeNull()
    expect(writes).toEqual(['mock-github-pw', ''])

    stop()
  })

  it('размонтирование закрывает всё открытое', async () => {
    const { secrets, stop } = run()
    await secrets.reveal('password')

    stop()

    expect(secrets.shown.password).toBeNull()
    expect(secrets.hideIn.password).toBe(0)
  })
})
