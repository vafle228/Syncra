import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import {
  createMockCoreClient,
  MOCK_DEVICE_PHONE,
  MOCK_RECORD_GITHUB,
  MOCK_VAULT_WORK,
  type MockCoreClient,
} from '@/core/mock'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useVaultStore } from '@/stores/useVaultStore'
import HomeView from '../HomeView.vue'

const push = vi.fn()

vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
}))

beforeEach(() => {
  setActivePinia(createPinia())
  push.mockClear()
})

afterEach(() => {
  setCoreClient(null)
})

/** Смонтировать экран на открытом хранилище и дождаться ответа мок-ядра. */
async function mountHome(
  core: MockCoreClient = createMockCoreClient({
    latencyMs: 0,
    startUnlocked: true,
  }),
) {
  setCoreClient(core)
  const wrapper = mount(HomeView)
  await flushPromises()
  return wrapper
}

async function search(wrapper: Awaited<ReturnType<typeof mountHome>>, value: string) {
  const input = wrapper.find<HTMLInputElement>('.home__search-input')
  input.element.value = value
  await input.trigger('input')
}

describe('HomeView · список (сквозной путь UI → IPC → мок-ядро)', () => {
  it('рендерит метаданные записей из мок-ядра', async () => {
    const wrapper = await mountHome()

    const text = wrapper.text()
    expect(text).toContain('Google')
    expect(text).toContain('personal.demo@gmail.com')
    expect(text).toContain('Рабочий')
  })

  it('не выводит на экран ни одного секрета', async () => {
    const wrapper = await mountHome()

    // Все сид-секреты помечены префиксом `mock-` / MOCKTOTP — ничего из этого
    // не должно попасть в разметку списка (Закон №1).
    expect(wrapper.html()).not.toMatch(/mock-[a-z]+-pw/)
    expect(wrapper.html()).not.toContain('MOCKTOTPSECRET')
    expect(wrapper.html()).not.toContain('Recovery codes')
  })

  it('не показывает удалённую запись (tombstone)', async () => {
    const wrapper = await mountHome()

    expect(wrapper.text()).not.toContain('Jira')
    expect(wrapper.findAll('.home__group-list li')).toHaveLength(4)
  })

  it('показывает счётчик записей и сервисов', async () => {
    const wrapper = await mountHome()

    expect(wrapper.find('.home__count').text()).toContain('4 записи · 3 сервиса')
  })

  it('показывает скелет, пока ядро не ответило', async () => {
    setCoreClient(createMockCoreClient({ latencyMs: 5, startUnlocked: true }))

    const wrapper = mount(HomeView)

    expect(wrapper.find('.home__skeleton').exists()).toBe(true)
    expect(wrapper.find('.home__list').exists()).toBe(false)

    await flushPromises()
    await new Promise((resolve) => setTimeout(resolve, 10))
    await flushPromises()

    expect(wrapper.find('.home__skeleton').exists()).toBe(false)
    expect(wrapper.find('.home__list').exists()).toBe(true)
  })

  it('показывает сообщение ядра вместо списка, если ядро вернуло ошибку', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    const wrapper = await mountHome(core)

    // `failNext` роняет БЛИЖАЙШУЮ команду, а при открытии экрана их несколько
    // (записи, секции, конфликты, статус синхронизации). Поэтому роняем именно
    // перезагрузку списка — проверяется путь «ядро отказало → вместо списка
    // его сообщение», а не то, какая команда уходит первой.
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    await useRecordsStore().load()
    await flushPromises()

    expect(wrapper.text()).toContain('Ядро недоступно.')
    expect(wrapper.find('.home__list').exists()).toBe(false)
    expect(wrapper.find('.home__skeleton').exists()).toBe(false)
  })

  it('показывает пустое состояние на свежесозданном хранилище', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })
    setCoreClient(core)
    await core.initVault('рыжий трамвай у моста')

    const wrapper = mount(HomeView)
    await flushPromises()

    expect(wrapper.text()).toContain('Пока ни одного пароля')
    expect(wrapper.find('.home__list').exists()).toBe(false)
  })
})

describe('HomeView · синхронизация и конфликты (F10, F11)', () => {
  it('показывает индикатор в шапке — по состоянию из ядра', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    const wrapper = await mountHome(core)

    // В сиде есть конфликт — он важнее всего остального (§ «Состояния»).
    expect(wrapper.find('.sync-chip').text()).toBe('1 конфликт')

    await useConflictsStore().resolve(MOCK_RECORD_GITHUB, 'local')
    await flushPromises()
    // Конфликта больше нет, но выбор ещё не уехал.
    expect(wrapper.find('.sync-chip').text()).toBe('1 изменение ждёт')

    core.control.peerFound(MOCK_DEVICE_PHONE)
    core.control.startSync(MOCK_DEVICE_PHONE)
    core.control.finishSync()
    await flushPromises()
    expect(wrapper.find('.sync-chip').text()).toBe('Синхронизировано')
  })

  it('помечает в списке запись, из-за которой спор, и ту, что ещё не уехала', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    const wrapper = await mountHome(core)

    const conflictRow = wrapper
      .findAll('.home__group-list li')
      .find((row) => row.text().includes('GitHub'))
    expect(conflictRow?.find('.sy-list-item__status--conflict').text()).toContain('конфликт версий')

    // Правка другой записи ждёт отправки — и это видно на её строке.
    const steam = useRecordsStore().records.find((record) => record.service_name === 'Steam')!
    await useRecordsStore().update(steam.record_id, { login: 'demo_player_2' })
    await flushPromises()

    const pendingRow = wrapper
      .findAll('.home__group-list li')
      .find((row) => row.text().includes('Steam'))
    expect(pendingRow?.find('.sy-list-item__status--pending').text()).toContain(
      'ждёт синхронизации',
    )
  })
})

describe('HomeView · группировка нескольких аккаунтов (§4.4)', () => {
  it('показывает заголовок группы там, где у сервиса несколько аккаунтов', async () => {
    const wrapper = await mountHome()

    const multi = wrapper.findAll('.home__group--multi')
    expect(multi).toHaveLength(1)

    const head = multi[0]?.find('.home__group-head')
    expect(head?.text()).toContain('Google')
    expect(head?.text()).toContain('2 аккаунта')
    expect(multi[0]?.findAll('li')).toHaveLength(2)
  })

  it('одиночная запись остаётся обычной строкой, без заголовка группы', async () => {
    const wrapper = await mountHome()

    const heads = wrapper.findAll('.home__group-head')
    expect(heads).toHaveLength(1)
    expect(heads[0]?.text()).toContain('Google')

    // GitHub и Steam — по одному аккаунту, заголовков у них нет.
    expect(wrapper.text()).toContain('GitHub')
    expect(wrapper.text()).toContain('Steam')
  })
})

describe('HomeView · поиск', () => {
  it('сужает список по имени сервиса', async () => {
    const wrapper = await mountHome()

    await search(wrapper, 'github')

    expect(wrapper.findAll('.home__group-list li')).toHaveLength(1)
    expect(wrapper.text()).toContain('demo-user')
    expect(wrapper.text()).not.toContain('Steam')
    expect(wrapper.find('.home__count').text()).toContain('найдено 1 из 4')
  })

  it('ищет по логину и по адресу сайта', async () => {
    const wrapper = await mountHome()

    await search(wrapper, 'demo_player')
    expect(wrapper.text()).toContain('Steam')

    await search(wrapper, 'admin.google.com')
    expect(wrapper.findAll('.home__group-list li')).toHaveLength(1)
    expect(wrapper.text()).toContain('work.demo@syncra.example')
  })

  it('различает аккаунты одного сервиса по метке', async () => {
    const wrapper = await mountHome()

    await search(wrapper, 'google личный')

    expect(wrapper.findAll('.home__group-list li')).toHaveLength(1)
    expect(wrapper.text()).toContain('personal.demo@gmail.com')
    expect(wrapper.text()).not.toContain('work.demo@syncra.example')
    // Совпал один аккаунт — заголовок группы не нужен.
    expect(wrapper.find('.home__group-head').exists()).toBe(false)
  })

  it('объясняет пустую выдачу и позволяет сбросить поиск', async () => {
    const wrapper = await mountHome()

    await search(wrapper, 'dropbox')

    expect(wrapper.text()).toContain('Ничего не нашлось')
    expect(wrapper.text()).toContain('«dropbox»')
    // Обещание продукта: поиск не заглядывает в секреты и не ходит в сеть.
    expect(wrapper.text()).toContain('не заглядывает в секреты')
    expect(wrapper.find('.home__list').exists()).toBe(false)

    await wrapper.find('.sy-empty__actions button').trigger('click')
    await flushPromises()

    expect(wrapper.find<HTMLInputElement>('.home__search-input').element.value).toBe('')
    expect(wrapper.findAll('.home__group-list li')).toHaveLength(4)
  })

  it('поиск не находит секретов — их нечем индексировать', async () => {
    const wrapper = await mountHome()

    await search(wrapper, 'mock-github-pw')

    expect(wrapper.text()).toContain('Ничего не нашлось')
  })
})

describe('HomeView · выбор записи и панели', () => {
  it('не открывает ничью карточку сама', async () => {
    const wrapper = await mountHome()

    expect(wrapper.text()).toContain('Запись не выбрана')
    expect(wrapper.find('.card').exists()).toBe(false)
  })

  it('открывает карточку по клику на строку', async () => {
    const wrapper = await mountHome()

    await wrapper.findAll('.home__row')[0]!.trigger('click')
    await flushPromises()

    const card = wrapper.find('.card')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('GitHub')
    // Открытая карточка не показывает секретов, пока их не попросили.
    expect(wrapper.html()).not.toMatch(/mock-[a-z]+-pw/)
  })

  it('переключает панель в форму по «Изменить» и обратно по «Отмена»', async () => {
    const wrapper = await mountHome()
    await wrapper.findAll('.home__row')[0]!.trigger('click')
    await flushPromises()

    await wrapper.find('.card__head-actions button').trigger('click')
    await flushPromises()
    expect(wrapper.find('.form__title').text()).toBe('Изменить запись')

    await wrapper.find('.form__head-actions button').trigger('click')
    await flushPromises()
    expect(wrapper.find('.card').exists()).toBe(true)
  })

  it('заводит новую запись сквозным путём: форма → ядро → список', async () => {
    const wrapper = await mountHome()

    await wrapper.find('.home__new').trigger('click')
    await flushPromises()
    expect(wrapper.find('.form__title').text()).toBe('Новая запись')

    const inputs = wrapper.findAll<HTMLInputElement>('.form__grid .sy-input input')
    const set = async (index: number, value: string) => {
      const input = inputs[index]!
      input.element.value = value
      await input.trigger('input')
    }
    // Порядок полей в сетке: сервис, логин, адрес, метка.
    await set(0, 'Figma')
    await set(1, 'anna@studio.example')
    await set(2, 'https://figma.com')

    const password = wrapper.find<HTMLInputElement>('.form__secrets input[type="password"]')
    password.element.value = 'mock-figma-pw'
    await password.trigger('input')

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    // Запись появилась в списке, панель показывает уже её карточку.
    expect(wrapper.findAll('.home__group-list li')).toHaveLength(5)
    expect(wrapper.find('.card__title').text()).toBe('Figma')
    expect(wrapper.find('.home__count').text()).toContain('5 записей · 4 сервиса')
    expect(wrapper.html()).not.toContain('mock-figma-pw')
  })

  it('после удаления запись уходит из списка, а панель — в пустое состояние', async () => {
    const wrapper = await mountHome()
    await wrapper.findAll('.home__row')[0]!.trigger('click')
    await flushPromises()

    await wrapper.find('.card__foot .sy-button--danger').trigger('click')
    await flushPromises()
    const confirm = [...document.body.querySelectorAll('.sy-modal__actions button')].find((node) =>
      node.textContent?.includes('Удалить запись'),
    ) as HTMLButtonElement
    confirm.click()
    await flushPromises()

    expect(wrapper.findAll('.home__group-list li')).toHaveLength(3)
    expect(wrapper.text()).not.toContain('demo-user')
    expect(wrapper.text()).toContain('Запись не выбрана')
  })
})

describe('HomeView · сайдбар секций (F7)', () => {
  /** Найти строку сайдбара по имени секции. */
  function section(wrapper: Awaited<ReturnType<typeof mountHome>>, name: string) {
    const found = wrapper
      .findAll('.sections__item')
      .find((node) => node.find('.sections__name').text() === name)
    if (!found) throw new Error(`Секция «${name}» не найдена в сайдбаре`)
    return found
  }

  it('показывает секции ядра со счётчиками и пометкой локальной', async () => {
    const wrapper = await mountHome()

    expect(section(wrapper, 'Все записи').text()).toContain('4')
    expect(section(wrapper, 'Личное').text()).toContain('3')
    expect(section(wrapper, 'Рабочее').text()).toContain('1')
    // «Рабочее» не синхронизируется — это видно там же, где её открывают.
    expect(section(wrapper, 'Рабочее').text()).toContain('локально')
    expect(section(wrapper, 'Личное').text()).not.toContain('локально')

    wrapper.unmount()
  })

  it('фильтрует список по выбранной секции', async () => {
    const wrapper = await mountHome()

    await section(wrapper, 'Рабочее').trigger('click')

    expect(wrapper.findAll('.home__group-list li')).toHaveLength(1)
    expect(wrapper.text()).toContain('work.demo@syncra.example')
    expect(wrapper.text()).not.toContain('demo-user')
    expect(wrapper.find('.home__count').text()).toContain('1 запись')

    await section(wrapper, 'Все записи').trigger('click')
    expect(wrapper.findAll('.home__group-list li')).toHaveLength(4)

    wrapper.unmount()
  })

  it('предупреждает, что записи локальной секции никуда не уезжают', async () => {
    const wrapper = await mountHome()

    await section(wrapper, 'Рабочее').trigger('click')
    expect(wrapper.find('.home__local').text()).toContain('не уезжают с этого устройства')

    await section(wrapper, 'Личное').trigger('click')
    expect(wrapper.find('.home__local').exists()).toBe(false)

    wrapper.unmount()
  })

  it('объясняет пустую секцию и даёт вернуться ко всем записям', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    const wrapper = await mountHome(core)
    const empty = await core.createVault('Учёба', 'mint')
    await useSectionsStore().load()
    await flushPromises()

    await section(wrapper, 'Учёба').trigger('click')

    expect(wrapper.text()).toContain('В этой секции пока пусто')
    expect(wrapper.find('.home__list').exists()).toBe(false)
    expect(useRecordsStore().vaultFilter).toBe(empty.vault_id)

    const back = wrapper
      .findAll('.sy-empty__actions button')
      .find((n) => n.text() === 'Показать все')
    await back!.trigger('click')
    expect(wrapper.findAll('.home__group-list li')).toHaveLength(4)

    wrapper.unmount()
  })

  it('новая запись ложится в открытую секцию', async () => {
    const wrapper = await mountHome()
    await wrapper.findAll('.sections__item')[2]!.trigger('click')

    await wrapper.find('.home__new').trigger('click')
    await flushPromises()

    const inputs = wrapper.findAll<HTMLInputElement>('.form__grid .sy-input input')
    const set = async (index: number, value: string) => {
      const input = inputs[index]!
      input.element.value = value
      await input.trigger('input')
    }
    await set(0, 'Figma')
    await set(1, 'anna@studio.example')

    const password = wrapper.find<HTMLInputElement>('.form__secrets input[type="password"]')
    password.element.value = 'mock-figma-pw'
    await password.trigger('input')

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    const created = useRecordsStore().records.find((record) => record.service_name === 'Figma')
    expect(created?.vault_id).toBe(MOCK_VAULT_WORK)

    wrapper.unmount()
  })
})

describe('HomeView · блокировка', () => {
  it('блокирует хранилище и уводит на экран входа', async () => {
    const wrapper = await mountHome()

    await wrapper.find('.home__header-actions button:last-child').trigger('click')
    await flushPromises()

    expect(useVaultStore().status).toBe('locked')
    expect(push).toHaveBeenCalledWith({ name: 'unlock' })
  })
})
