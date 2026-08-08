import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'

import { PREVIEW_DEBOUNCE_MS } from '@/composables/usePasswordGenerator'
import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, DEFAULT_GENERATOR_PROFILE, type MockCoreClient } from '@/core/mock'
import { useGeneratorStore } from '@/stores/useGeneratorStore'
import { useToastStore } from '@/stores/useToastStore'
import SettingsView from '../SettingsView.vue'

/**
 * Экран настроек (F6, §3.11): профиль генератора настраивается ОДИН РАЗ, а
 * пример рядом показывает, к чему приводят выбранные правила.
 */

let core: MockCoreClient
const writes: string[] = []

beforeEach(() => {
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

afterEach(() => {
  setCoreClient(null)
  vi.unstubAllGlobals()
})

/** RouterLink компонента здесь не нужен — экран проверяем без роутера. */
async function mountView() {
  const wrapper = mount(SettingsView, {
    global: { stubs: { RouterLink: { template: '<a><slot /></a>' } } },
  })
  await flushPromises()
  return wrapper
}

function example(wrapper: VueWrapper): string {
  return wrapper.find('.settings__example').text()
}

function button(wrapper: VueWrapper, label: string) {
  const found = wrapper.findAll('button').find((node) => node.text() === label)
  if (!found) throw new Error(`Кнопка «${label}» не найдена`)
  return found
}

describe('SettingsView', () => {
  it('показывает правила и пример по ним', async () => {
    const wrapper = await mountView()

    expect(wrapper.text()).toContain('Профиль генератора')
    expect(wrapper.text()).toContain('Пример по текущим правилам')
    expect(example(wrapper)).toHaveLength(DEFAULT_GENERATOR_PROFILE.length)
    expect(wrapper.text()).toMatch(/≈ \d+ бит/)

    wrapper.unmount()
  })

  it('«Другой пример» пересобирает пароль по тем же правилам', async () => {
    const wrapper = await mountView()
    const before = example(wrapper)

    await button(wrapper, 'Другой пример').trigger('click')
    await flushPromises()

    expect(example(wrapper)).not.toBe(before)
    expect(example(wrapper)).toHaveLength(DEFAULT_GENERATOR_PROFILE.length)

    wrapper.unmount()
  })

  it('правка правил меняет пример ещё до сохранения', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SettingsView, {
      global: { stubs: { RouterLink: { template: '<a><slot /></a>' } } },
    })
    await vi.runOnlyPendingTimersAsync()
    await flushPromises()

    await button(wrapper, 'слова').trigger('click')
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS)
    await flushPromises()

    // Фраза из слов, а не набор символов — правило применилось к примеру...
    expect(example(wrapper).split('-')).toHaveLength(DEFAULT_GENERATOR_PROFILE.words)
    // ...но в ядре пока лежит прежний профиль: «Сохранить» ещё не нажимали.
    await expect(core.getGeneratorProfile()).resolves.toEqual(DEFAULT_GENERATOR_PROFILE)

    wrapper.unmount()
    vi.useRealTimers()
  })

  it('сохраняет правила в ядро по явному нажатию (§6.1)', async () => {
    const wrapper = await mountView()

    await button(wrapper, 'слова').trigger('click')
    await button(wrapper, 'Сохранить как профиль').trigger('click')
    await flushPromises()

    await expect(core.getGeneratorProfile()).resolves.toMatchObject({ mode: 'words' })
    expect(useGeneratorStore().profile).toMatchObject({ mode: 'words' })

    wrapper.unmount()
  })

  it('показывает сообщение ядра, если сохранить не удалось', async () => {
    const wrapper = await mountView()
    core.control.failNext('INTERNAL', 'Хранилище занято.')

    await button(wrapper, 'слова').trigger('click')
    await button(wrapper, 'Сохранить как профиль').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Хранилище занято.')
    // Настройки остались на экране — их не смахнуло сообщением об ошибке.
    expect(wrapper.find('.gp').exists()).toBe(true)

    wrapper.unmount()
  })

  it('копирует пример как секрет: с очисткой буфера', async () => {
    const wrapper = await mountView()
    const shown = example(wrapper)

    await button(wrapper, 'Скопировать').trigger('click')
    await flushPromises()

    expect(writes).toEqual([shown])
    // Обещание про самоочистку буфера пользователю проговаривается (§4.5).
    const toasts = useToastStore().toasts
    expect(toasts[toasts.length - 1]?.text).toContain('очистится через 20 с')

    wrapper.unmount()
  })

  it('ЗАКОН №1: пример не попадает в Pinia', async () => {
    const wrapper = await mountView()
    const shown = example(wrapper)

    expect(JSON.stringify(useGeneratorStore().$state)).not.toContain(shown)

    wrapper.unmount()
  })
})

describe('SettingsView · данные (F12)', () => {
  it('ставит ценник у каждого действия, а не прячет опасное', async () => {
    const wrapper = await mountView()
    const rows = wrapper.findAll('.settings__data-row')

    expect(rows).toHaveLength(3)
    expect(rows[0]!.text()).toContain('Импорт из другого менеджера')
    expect(rows[1]!.text()).toContain('Зашифрованный бэкап')
    // Опасное на виду и помечено словами, а не только цветом.
    expect(rows[2]!.text()).toContain('Экспорт в CSV')
    expect(rows[2]!.text()).toContain('открытый текст')
    expect(rows[2]!.classes()).toContain('settings__data-row--danger')

    wrapper.unmount()
  })
})
