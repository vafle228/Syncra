import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'

import { securityPolicy } from '@/composables/securityPolicy'
import { PREVIEW_DEBOUNCE_MS } from '@/composables/usePasswordGenerator'
import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, DEFAULT_GENERATOR_PROFILE, type MockCoreClient } from '@/core/mock'
import { useGeneratorStore } from '@/stores/useGeneratorStore'
import { useToastStore } from '@/stores/useToastStore'
import { mountWithRouter } from '@/test/mountWithRouter'
import SettingsView from '../SettingsView.vue'

/**
 * Экран настроек (F6, F13, §3.11): три вкладки одной правой панели.
 *
 * Профиль генератора настраивается ОДИН РАЗ, а пример рядом показывает, к чему
 * приводят выбранные правила. С F13 к нему добавились вкладки «Безопасность»
 * (таймауты замка, буфера и показа) и «Данные» (импорт, бэкап, CSV).
 */

let core: MockCoreClient
const writes: string[] = []

beforeEach(() => {
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

/**
 * Роутер настоящий: адрес — единственная правда о том, какая вкладка открыта,
 * и проверять переключение шпионом значило бы проверять не её.
 */
async function mountView(path = '/settings') {
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  const harness = await mountWithRouter(SettingsView, { core, path })
  await flushPromises()
  return harness
}

/** Открыть вкладку по её ярлыку. */
async function openTab(wrapper: VueWrapper, id: 'security' | 'generator' | 'data') {
  await wrapper.find(`[data-test="tab-${id}"]`).trigger('click')
  await flushPromises()
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
    const { wrapper } = await mountView('/settings?tab=generator')

    expect(wrapper.text()).toContain('Профиль генератора')
    expect(wrapper.text()).toContain('Пример по текущим правилам')
    expect(example(wrapper)).toHaveLength(DEFAULT_GENERATOR_PROFILE.length)
    expect(wrapper.text()).toMatch(/≈ \d+ бит/)

    wrapper.unmount()
  })

  it('«Другой пример» пересобирает пароль по тем же правилам', async () => {
    const { wrapper } = await mountView('/settings?tab=generator')
    const before = example(wrapper)

    await button(wrapper, 'Другой пример').trigger('click')
    await flushPromises()

    expect(example(wrapper)).not.toBe(before)
    expect(example(wrapper)).toHaveLength(DEFAULT_GENERATOR_PROFILE.length)

    wrapper.unmount()
  })

  it('правка правил меняет пример ещё до сохранения', async () => {
    const { wrapper } = await mountView('/settings?tab=generator')
    vi.useFakeTimers()

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
    const { wrapper } = await mountView('/settings?tab=generator')

    await button(wrapper, 'слова').trigger('click')
    await button(wrapper, 'Сохранить как профиль').trigger('click')
    await flushPromises()

    await expect(core.getGeneratorProfile()).resolves.toMatchObject({ mode: 'words' })
    expect(useGeneratorStore().profile).toMatchObject({ mode: 'words' })

    wrapper.unmount()
  })

  it('показывает сообщение ядра, если сохранить не удалось', async () => {
    const { wrapper } = await mountView('/settings?tab=generator')
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
    const { wrapper } = await mountView('/settings?tab=generator')
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
    const { wrapper } = await mountView('/settings?tab=generator')
    const shown = example(wrapper)

    expect(JSON.stringify(useGeneratorStore().$state)).not.toContain(shown)

    wrapper.unmount()
  })
})

describe('SettingsView · вкладки (F13)', () => {
  it('по умолчанию открывает «Безопасность»', async () => {
    // Первой стоит единственная вкладка, где выбор меняет то, КАК продукт
    // защищает человека. Остальные — про удобство и редкие операции.
    const { wrapper } = await mountView()

    expect(wrapper.find('[data-test="pane-security"]').exists()).toBe(true)
    expect(wrapper.find('[data-test="tab-security"]').attributes('aria-selected')).toBe('true')
  })

  it('вкладка живёт в адресе, а не только в состоянии', async () => {
    const { wrapper, router } = await mountView()

    await openTab(wrapper, 'data')

    expect(router.currentRoute.value.query.tab).toBe('data')
    expect(wrapper.find('[data-test="pane-data"]').exists()).toBe(true)
  })

  it('принимает старые ссылки `/data?tab=import`', async () => {
    // До F13 импорт и экспорт жили на отдельном экране; закладки должны открыть
    // ту вкладку, ради которой их сохраняли.
    const { wrapper } = await mountView('/settings?tab=import')

    expect(wrapper.find('[data-test="pane-data"]').exists()).toBe(true)
  })
})

describe('SettingsView · безопасность (F13)', () => {
  it('показывает три таймаута и отмечает действующее значение', async () => {
    const { wrapper } = await mountView()

    const rows = wrapper.findAll('.settings__option')
    expect(rows).toHaveLength(3)
    expect(rows[0]!.text()).toContain('Автоблокировка')
    expect(rows[1]!.text()).toContain('Очистка буфера обмена')
    expect(rows[2]!.text()).toContain('Показ секрета на экране')

    // Умолчание ядра — средняя ступень каждой тройки.
    expect(rows[0]!.find('.settings__choice--on').text()).toBe('5 мин')
    expect(rows[1]!.find('.settings__choice--on').text()).toBe('20 с')
    expect(rows[2]!.find('.settings__choice--on').text()).toBe('30 с')
  })

  it('выбор доезжает до ядра и становится действующей политикой', async () => {
    const { wrapper } = await mountView()

    const clipboardRow = wrapper.findAll('.settings__option')[1]!
    // 60 000 мс подписаны как «1 мин»: секунды и минуты — разные единицы, и
    // писать «60 с» там, где человек читает минуту, значит считать за него.
    const choice = clipboardRow.findAll('.settings__choice').find((n) => n.text() === '1 мин')!
    await choice.trigger('click')
    await flushPromises()

    await expect(core.getSecuritySettings()).resolves.toMatchObject({ clipboard_clear_ms: 60_000 })
    // И таймеры показа/копирования читают уже новое число.
    expect(securityPolicy().value.clipboard_clear_ms).toBe(60_000)
  })

  it('объясняет, что автоблокировку считает ядро, а не окно', async () => {
    // Второй таймер во фронте означал бы вторую правду о том, заперто ли
    // хранилище, — поэтому его нет, и текст обещает ровно то, что есть.
    const { wrapper } = await mountView()

    expect(wrapper.findAll('.settings__option')[0]!.text()).toContain('Считает ядро')
  })

  it('открывает смену мастер-пароля', async () => {
    const { wrapper } = await mountView()

    expect(document.body.querySelector('[data-test="master-password-modal"]')).toBeNull()

    await wrapper.find('[data-test="master-password-open"]').trigger('click')
    await flushPromises()

    expect(document.body.querySelector('[data-test="master-password-modal"]')).not.toBeNull()
  })
})

describe('SettingsView · данные (F12)', () => {
  it('ставит ценник у каждого действия, а не прячет опасное', async () => {
    const { wrapper } = await mountView('/settings?tab=data')
    const rows = wrapper.findAll('.settings__data-row')

    expect(rows).toHaveLength(3)
    expect(rows[0]!.text()).toContain('Импорт из другого менеджера')
    expect(rows[1]!.text()).toContain('Зашифрованный бэкап')
    // Опасное на виду и помечено словами, а не только цветом.
    expect(rows[2]!.text()).toContain('Экспорт в CSV')
    expect(rows[2]!.text()).toContain('не шифруется')
    expect(rows[2]!.classes()).toContain('settings__data-row--danger')
  })

  it('обещает, что ни импорт, ни экспорт не ходят в сеть', async () => {
    const { wrapper } = await mountView('/settings?tab=data')

    expect(wrapper.find('.settings__data-note').text()).toContain('не обращаются к сети')
  })

  it('каждая карточка открывает свою модалку', async () => {
    const { wrapper } = await mountView('/settings?tab=data')

    await wrapper.find('[data-test="open-csv"]').trigger('click')
    await flushPromises()
    expect(document.body.querySelector('[data-test="csv-modal"]')).not.toBeNull()
    // Дословный текст риска — `CLAUDE.md` запрещает его смягчать.
    expect(document.body.textContent).toContain('пароли внутри читаются как обычный текст')
  })

  it('бэкап и импорт тоже открываются, и до открытия их содержимого нет', async () => {
    // Содержимое под `v-if`: мастер импорта держит сеанс в ядре, и оставлять
    // его смонтированным за закрытым диалогом значило бы держать открытым
    // разобранный чужой файл.
    const { wrapper } = await mountView('/settings?tab=data')

    expect(document.body.querySelector('[data-test="backup-modal"]')).toBeNull()
    expect(document.body.querySelector('[data-test="import-modal"]')).toBeNull()

    await wrapper.find('[data-test="open-backup"]').trigger('click')
    await flushPromises()
    expect(document.body.querySelector('[data-test="backup-modal"]')).not.toBeNull()

    await wrapper.find('[data-test="open-import"]').trigger('click')
    await flushPromises()
    expect(document.body.querySelector('[data-test="import-modal"]')).not.toBeNull()
  })
})
