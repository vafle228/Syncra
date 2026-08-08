import { afterEach, describe, expect, it } from 'vitest'
import { flushPromises } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import type { MockCoreClient } from '@/core/mock'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useVaultUiStore } from '@/stores/useVaultUiStore'
import { mountWithRouter } from '@/test/mountWithRouter'
import VaultView from '../VaultView.vue'

/**
 * Правая панель главного экрана (F4, F5, F13) — вторая половина бывшего
 * `HomeView`. Монтируется напрямую: `RouterView` внутри неё нет.
 */

afterEach(() => {
  setCoreClient(null)
})

async function mountPane(core?: MockCoreClient) {
  const harness = await mountWithRouter(VaultView, { core })
  await useRecordsStore().load()
  await flushPromises()
  return harness
}

/** Найти запись сида по имени сервиса. */
function recordId(service: string): string {
  const found = useRecordsStore().records.find((record) => record.service_name === service)
  if (!found) throw new Error(`Запись «${service}» не найдена`)
  return found.record_id
}

describe('VaultView · что показывает панель', () => {
  it('не открывает ничью карточку сама', async () => {
    const { wrapper } = await mountPane()

    // Открывать чужой пароль без спроса — не то, чего ждёшь от менеджера
    // паролей на общем столе.
    expect(wrapper.text()).toContain('Запись не выбрана')
    expect(wrapper.find('.card').exists()).toBe(false)
  })

  it('показывает карточку выбранной записи и ни одного секрета в ней', async () => {
    const { wrapper } = await mountPane()

    useVaultUiStore().openRecord(recordId('GitHub'))
    await flushPromises()

    const card = wrapper.find('.card')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('GitHub')
    expect(wrapper.html()).not.toMatch(/mock-[a-z]+-pw/)
  })

  it('переключается в форму по «Изменить» и обратно по «Отмена»', async () => {
    const { wrapper } = await mountPane()
    useVaultUiStore().openRecord(recordId('GitHub'))
    await flushPromises()

    await wrapper.find('.card__head-actions button').trigger('click')
    await flushPromises()
    expect(wrapper.find('.form__title').text()).toBe('Изменить запись')

    await wrapper.find('.form__head-actions button').trigger('click')
    await flushPromises()
    expect(wrapper.find('.card').exists()).toBe(true)
  })

  it('показывает форму создания и не правит при этом чужую запись', async () => {
    const { wrapper } = await mountPane()
    useVaultUiStore().openRecord(recordId('GitHub'))
    await flushPromises()

    useVaultUiStore().startCreate()
    await flushPromises()

    expect(wrapper.find('.form__title').text()).toBe('Новая запись')
    // Выбор снят: форма создания ничью карточку не редактирует.
    expect(useRecordsStore().selectedId).toBeNull()
  })

  it('пустое состояние заводит новую запись', async () => {
    const { wrapper } = await mountPane()

    await wrapper.find('.sy-empty__actions button').trigger('click')
    await flushPromises()

    expect(wrapper.find('.form__title').text()).toBe('Новая запись')
  })
})

describe('VaultView · уход с экрана', () => {
  it('закрывает редактор, когда панель размонтируют', async () => {
    // В приложении это переход в «Настройки»: вернувшись, человек ждёт список,
    // а не забытый наполовину заполненный черновик.
    const { wrapper } = await mountPane()
    useVaultUiStore().startCreate()
    await flushPromises()
    expect(useVaultUiStore().editor).toBe('create')

    wrapper.unmount()

    expect(useVaultUiStore().editor).toBe('none')
  })
})
