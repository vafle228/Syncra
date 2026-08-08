import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { effectScope, ref } from 'vue'

import { DEFAULT_SECURITY_SETTINGS, type RecordId } from '@/core/contract'
import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_RECORD_GITHUB, type MockCoreClient } from '@/core/mock'
import { useConflictsStore } from '@/stores/useConflictsStore'

import { resetSecurityPolicy, setSecurityPolicy } from '../securityPolicy'
import { useConflictSecrets } from '../useConflictSecrets'

/**
 * ЗАКОН №1 при разрешении конфликта (F11): значения двух версий приходят
 * разово, живут ровно пока показаны и не остаются ни в Pinia, ни после
 * блокировки хранилища.
 */

let core: MockCoreClient

function run(recordId: RecordId | null = MOCK_RECORD_GITHUB) {
  const scope = effectScope()
  const id = ref<RecordId | null>(recordId)
  const secrets = scope.run(() => useConflictSecrets(id))!
  return { secrets, id, stop: () => scope.stop() }
}

beforeEach(() => {
  vi.useFakeTimers()
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  core.control.dispose()
  setCoreClient(null)
  // Политика живёт в модуле: без сброса она протекла бы из теста в тест.
  resetSecurityPolicy()
  vi.useRealTimers()
})

describe('открытие пары значений', () => {
  it('показывает одно поле обеих версий — сравнивать нужно именно их', async () => {
    const { secrets, stop } = run()

    await secrets.reveal('password')

    expect(secrets.shown.password).toEqual({
      local: 'mock-github-pw',
      remote: 'mock-github-pw-phone',
    })
    // Остальные поля не просили — их и нет.
    expect(secrets.shown.notes).toBeNull()
    expect(secrets.shown.totp_secret).toBeNull()

    stop()
  })

  it('закрывается само через 30 секунд и отсчитывает их на экране', async () => {
    const { secrets, stop } = run()
    await secrets.reveal('password')

    expect(secrets.hideIn.password).toBe(DEFAULT_SECURITY_SETTINGS.secret_reveal_ms / 1000)
    await vi.advanceTimersByTimeAsync(DEFAULT_SECURITY_SETTINGS.secret_reveal_ms)

    expect(secrets.shown.password).toBeNull()
    expect(secrets.hideIn.password).toBe(0)

    stop()
  })

  it('слушается срока из настроек безопасности (F13)', async () => {
    // Сравнение версий подчиняется той же настройке, что и карточка: иначе
    // пароль оставался бы на экране дольше обещанного именно там, где рядом
    // открыты сразу две версии.
    setSecurityPolicy({ ...DEFAULT_SECURITY_SETTINGS, secret_reveal_ms: 15_000 })
    const { secrets, stop } = run()

    await secrets.reveal('password')
    expect(secrets.hideIn.password).toBe(15)

    await vi.advanceTimersByTimeAsync(15_000)
    expect(secrets.shown.password).toBeNull()

    stop()
  })

  it('закрывается по повторному нажатию', async () => {
    const { secrets, stop } = run()

    await secrets.toggle('notes')
    expect(secrets.shown.notes?.remote).toContain('mock-4444')

    await secrets.toggle('notes')
    expect(secrets.shown.notes).toBeNull()

    stop()
  })

  it('показывает сообщение ядра, а не пустые значения', async () => {
    core.control.failNext('INTERNAL', 'Хранилище занято.')
    const { secrets, stop } = run()

    await secrets.reveal('password')

    expect(secrets.error.value).toBe('Хранилище занято.')
    expect(secrets.shown.password).toBeNull()

    stop()
  })
})

describe('где значения НЕ остаются', () => {
  it('ни в одном сторе Pinia', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const conflicts = useConflictsStore()
    await conflicts.load()

    const { secrets, stop } = run()
    await secrets.reveal('password')

    expect(JSON.stringify(pinia.state.value)).not.toContain('mock-github-pw')

    stop()
  })

  it('исчезают, когда хранилище заперли', async () => {
    const { secrets, stop } = run()
    await secrets.reveal('password')

    core.control.forceLock('timeout')

    expect(secrets.shown.password).toBeNull()

    stop()
  })

  it('исчезают при смене записи и при уходе с экрана', async () => {
    const { secrets, id, stop } = run()
    await secrets.reveal('password')

    id.value = 'другая-запись'
    await vi.advanceTimersByTimeAsync(0)
    expect(secrets.shown.password).toBeNull()

    await secrets.reveal('password')
    stop()
    expect(secrets.shown.password).toBeNull()
  })
})
