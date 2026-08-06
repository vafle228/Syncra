import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'
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
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    const wrapper = await mountHome(core)

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

describe('HomeView · блокировка', () => {
  it('блокирует хранилище и уводит на экран входа', async () => {
    const wrapper = await mountHome()

    await wrapper.find('.home__header-actions button:last-child').trigger('click')
    await flushPromises()

    expect(useVaultStore().status).toBe('locked')
    expect(push).toHaveBeenCalledWith({ name: 'unlock' })
  })
})
