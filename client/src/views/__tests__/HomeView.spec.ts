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

describe('HomeView · блокировка', () => {
  it('блокирует хранилище и уводит на экран входа', async () => {
    const wrapper = await mountHome()

    await wrapper.find('.home__header-actions button:last-child').trigger('click')
    await flushPromises()

    expect(useVaultStore().status).toBe('locked')
    expect(push).toHaveBeenCalledWith({ name: 'unlock' })
  })
})
