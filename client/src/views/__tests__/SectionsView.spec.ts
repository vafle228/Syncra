import { afterEach, describe, expect, it } from 'vitest'
import { flushPromises } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_VAULT_PERSONAL, MOCK_VAULT_WORK } from '@/core/mock'
import type { MockCoreClient } from '@/core/mock'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { mountWithRouter, waitForRoute } from '@/test/mountWithRouter'
import SectionsView from '../SectionsView.vue'

/**
 * Экран управления секциями (F7). Проверяем оба обещания экрана: тумблер §4.2
 * доезжает до ядра, а удаление секции не уносит с собой записи.
 */

let core: MockCoreClient

afterEach(() => {
  setCoreClient(null)
})

/**
 * Роутер здесь настоящий: с F13 «Открыть» правда уводит к паролям, и проверять
 * это шпионом над `push` значило бы проверять не переход, а вызов.
 */
async function mountSections() {
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  const harness = await mountWithRouter(SectionsView, { core, path: '/sections' })
  await flushPromises()
  return harness
}

type Harness = Awaited<ReturnType<typeof mountSections>>
type View = Harness['wrapper']

function card(wrapper: View, name: string) {
  const found = wrapper
    .findAll('.sections-view__card')
    .find((node) => node.find('.sections-view__name').text() === name)
  if (!found) throw new Error(`Карточка секции «${name}» не найдена`)
  return found
}

function button(scope: ReturnType<View['find']>, text: string) {
  const found = scope.findAll('button').find((node) => node.text() === text)
  if (!found) throw new Error(`Кнопка «${text}» не найдена`)
  return found
}

describe('SectionsView · список секций', () => {
  it('показывает секции из ядра с их состоянием', async () => {
    const { wrapper } = await mountSections()

    // Три строки: системная «Все записи» и две настоящие секции.
    expect(wrapper.findAll('.sections-view__card')).toHaveLength(3)
    expect(wrapper.find('.sections-view__card--system').text()).toContain('Все записи')
    expect(card(wrapper, 'Личное').text()).toContain('по умолчанию')
    expect(card(wrapper, 'Личное').text()).toContain('синхронизируется')
    expect(card(wrapper, 'Рабочее').text()).toContain('остаётся на этом устройстве')

    wrapper.unmount()
  })

  it('считает записи в каждой секции', async () => {
    const { wrapper } = await mountSections()

    expect(card(wrapper, 'Личное').text()).toContain('3 записи')
    // Надгробие в «Рабочем» не считается за запись (§5.4).
    expect(card(wrapper, 'Рабочее').text()).toContain('1 запись')

    wrapper.unmount()
  })

  it('у секции по умолчанию нет кнопки удаления: записям нужно куда-то попадать', async () => {
    const { wrapper } = await mountSections()

    expect(
      card(wrapper, 'Личное')
        .findAll('button')
        .some((node) => node.text() === 'Удалить'),
    ).toBe(false)
    expect(
      card(wrapper, 'Рабочее')
        .findAll('button')
        .some((node) => node.text() === 'Удалить'),
    ).toBe(true)

    wrapper.unmount()
  })

  it('проговаривает, что локальная секция никуда не уезжает', async () => {
    const { wrapper } = await mountSections()

    expect(wrapper.text()).toContain('Локальная секция никуда не уезжает')
    expect(wrapper.text()).toContain('Удаление секции не удаляет записи')

    wrapper.unmount()
  })
})

describe('SectionsView · синхронизация (§4.2)', () => {
  it('тумблер доезжает до ядра и переживает перезагрузку экрана', async () => {
    const { wrapper } = await mountSections()
    const toggle = card(wrapper, 'Рабочее').find('.sy-toggle__switch')

    expect(toggle.attributes('aria-checked')).toBe('false')

    await toggle.trigger('click')
    await flushPromises()

    expect((await core.listVaults()).find((v) => v.vault_id === MOCK_VAULT_WORK)?.sync).toBe(true)
    expect(card(wrapper, 'Рабочее').find('.sy-toggle__switch').attributes('aria-checked')).toBe(
      'true',
    )
    expect(card(wrapper, 'Рабочее').text()).toContain('синхронизируется')

    wrapper.unmount()
  })

  it('показывает отказ ядра у той секции, на которой нажали', async () => {
    const { wrapper } = await mountSections()
    core.control.failNext('INTERNAL', 'Ядро занято.')

    await card(wrapper, 'Рабочее').find('.sy-toggle__switch').trigger('click')
    await flushPromises()

    expect(card(wrapper, 'Рабочее').text()).toContain('Ядро занято.')
    expect(card(wrapper, 'Личное').text()).not.toContain('Ядро занято.')
    // Тумблер остался в прежнем положении: состояние берётся из ответа ядра.
    expect(card(wrapper, 'Рабочее').find('.sy-toggle__switch').attributes('aria-checked')).toBe(
      'false',
    )

    wrapper.unmount()
  })
})

describe('SectionsView · создание и правка', () => {
  it('создаёт секцию сквозным путём: форма → ядро → список', async () => {
    const { wrapper } = await mountSections()

    await wrapper.find('.sections-view__new').trigger('click')
    await flushPromises()

    const input = wrapper.find<HTMLInputElement>('.section-editor input')
    input.element.value = 'Учёба'
    await input.trigger('input')
    await wrapper.find('.section-editor').trigger('submit')
    await flushPromises()

    expect((await core.listVaults()).map((vault) => vault.name)).toEqual([
      'Личное',
      'Рабочее',
      'Учёба',
    ])
    expect(card(wrapper, 'Учёба').text()).toContain('0 записей')
    // Новая секция синхронизируется, но по умолчанию не становится.
    expect(card(wrapper, 'Учёба').text()).toContain('синхронизируется')
    expect(card(wrapper, 'Учёба').find('.sections-view__tag').exists()).toBe(false)
    expect(card(wrapper, 'Личное').find('.sections-view__tag').text()).toBe('по умолчанию')

    wrapper.unmount()
  })

  it('не создаёт секцию без имени', async () => {
    const { wrapper } = await mountSections()

    await wrapper.find('.sections-view__new').trigger('click')
    await wrapper.find('.section-editor').trigger('submit')
    await flushPromises()

    expect(wrapper.text()).toContain('У секции должно быть имя.')
    expect(await core.listVaults()).toHaveLength(2)

    wrapper.unmount()
  })

  it('переименовывает секцию на месте карточки', async () => {
    const { wrapper } = await mountSections()

    await button(card(wrapper, 'Рабочее'), 'Переименовать').trigger('click')
    await flushPromises()

    const input = wrapper.find<HTMLInputElement>('.section-editor input')
    expect(input.element.value).toBe('Рабочее')
    input.element.value = 'Работа'
    await input.trigger('input')
    await wrapper.find('.section-editor').trigger('submit')
    await flushPromises()

    expect((await core.listVaults()).map((vault) => vault.name)).toContain('Работа')
    expect(wrapper.find('.section-editor').exists()).toBe(false)

    wrapper.unmount()
  })

  it('назначает секцию по умолчанию', async () => {
    const { wrapper } = await mountSections()

    await button(card(wrapper, 'Рабочее'), 'Сюда по умолчанию').trigger('click')
    await flushPromises()

    const vaults = await core.listVaults()
    expect(vaults.filter((vault) => vault.is_default).map((v) => v.vault_id)).toEqual([
      MOCK_VAULT_WORK,
    ])
    expect(card(wrapper, 'Рабочее').text()).toContain('сюда попадают новые')

    wrapper.unmount()
  })
})

describe('SectionsView · удаление', () => {
  it('обещает сохранить записи и правда их сохраняет', async () => {
    const { wrapper } = await mountSections()
    const list = useRecordsStore()
    const before = list.totalAll

    await button(card(wrapper, 'Рабочее'), 'Удалить').trigger('click')
    await flushPromises()

    const dialog = document.body.querySelector('.sy-modal__dialog')
    expect(dialog?.textContent).toContain('Удалить секцию «Рабочее»?')
    expect(dialog?.textContent).toContain('переедут в «Личное»')

    const confirm = [...document.body.querySelectorAll('.sy-modal__actions button')].find((node) =>
      node.textContent?.includes('Удалить секцию'),
    ) as HTMLButtonElement
    confirm.click()
    await flushPromises()

    expect((await core.listVaults()).map((vault) => vault.vault_id)).toEqual([MOCK_VAULT_PERSONAL])
    // Записей столько же, все переехали в секцию по умолчанию (§4.2).
    expect(list.totalAll).toBe(before)
    expect(list.records.every((record) => record.vault_id === MOCK_VAULT_PERSONAL)).toBe(true)
    expect(useSectionsStore().vaults).toHaveLength(1)

    wrapper.unmount()
  })

  it('снимает фильтр списка по удалённой секции', async () => {
    const { wrapper } = await mountSections()
    const list = useRecordsStore()
    list.setVaultFilter(MOCK_VAULT_WORK)

    await button(card(wrapper, 'Рабочее'), 'Удалить').trigger('click')
    await flushPromises()
    const confirm = [...document.body.querySelectorAll('.sy-modal__actions button')].find((node) =>
      node.textContent?.includes('Удалить секцию'),
    ) as HTMLButtonElement
    confirm.click()
    await flushPromises()

    expect(list.vaultFilter).toBeNull()

    wrapper.unmount()
  })

  it('оставляет секцию на месте, если ядро отказало', async () => {
    const { wrapper } = await mountSections()

    await button(card(wrapper, 'Рабочее'), 'Удалить').trigger('click')
    await flushPromises()
    core.control.failNext('INTERNAL', 'Хранилище занято.')
    const confirm = [...document.body.querySelectorAll('.sy-modal__actions button')].find((node) =>
      node.textContent?.includes('Удалить секцию'),
    ) as HTMLButtonElement
    confirm.click()
    await flushPromises()

    expect(document.body.querySelector('.sy-modal__dialog')?.textContent).toContain(
      'Хранилище занято.',
    )
    expect(await core.listVaults()).toHaveLength(2)

    wrapper.unmount()
  })
})

describe('SectionsView · ЗАКОН №1', () => {
  it('не показывает ни одного секрета', async () => {
    const { wrapper } = await mountSections()

    expect(wrapper.html()).not.toMatch(/mock-[a-z]+-pw/)
    expect(wrapper.html()).not.toContain('MOCKTOTPSECRET')
    expect(wrapper.html()).not.toContain('Recovery codes')

    wrapper.unmount()
  })
})

describe('SectionsView · открыть секцию (F13)', () => {
  it('«Открыть» ставит секцию фильтром и возвращает к паролям', async () => {
    // Ради этого секции и существуют; идти за тем же в сайдбар — лишний шаг
    // ровно там, где человек уже смотрит на нужную секцию.
    const { wrapper, router } = await mountSections()

    await button(card(wrapper, 'Рабочее'), 'Открыть').trigger('click')
    await flushPromises()

    expect(useRecordsStore().vaultFilter).toBe(MOCK_VAULT_WORK)
    await waitForRoute(router, 'home')
  })

  it('системная строка считает записи всех секций сразу', async () => {
    const { wrapper } = await mountSections()

    const system = wrapper.find('.sections-view__card--system')
    expect(system.text()).toContain('4 записи')
    expect(system.text()).toContain('системная')
    // Открывать и удалять там нечего: это не секция.
    expect(system.findAll('button')).toHaveLength(0)
  })
})
