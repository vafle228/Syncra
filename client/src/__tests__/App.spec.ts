import { afterEach, describe, expect, it } from 'vitest'
import { flushPromises } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_MASTER_PASSWORD } from '@/core/mock'
import { mountWithRouter, waitForRoute } from '@/test/mountWithRouter'
import App from '../App.vue'

/**
 * Корень приложения: всё лежит внутри окна (F13), включая экран блокировки.
 */

afterEach(() => {
  setCoreClient(null)
})

describe('App', () => {
  it('рисует окно с полосой заголовка и выход роутера внутри', async () => {
    const { wrapper } = await mountWithRouter(App)

    expect(wrapper.find('[data-test="titlebar"]').exists()).toBe(true)
    // Внутри окна — реальный экран, а не заглушка: роутер настоящий.
    expect(wrapper.find('.home').exists()).toBe(true)
  })

  it('полоса заголовка говорит, заперто ли хранилище', async () => {
    const { wrapper, router } = await mountWithRouter(App, {
      core: createMockCoreClient({ latencyMs: 0 }),
    })

    // Хранилище закрыто — хранитель увёл на экран входа.
    expect(router.currentRoute.value.name).toBe('unlock')
    expect(wrapper.find('[data-test="titlebar"]').text()).toContain('Syncra — заперто')
  })

  it('после разблокировки заголовок меняется', async () => {
    const { wrapper, core } = await mountWithRouter(App, {
      core: createMockCoreClient({ latencyMs: 0 }),
    })

    await core.unlock(MOCK_MASTER_PASSWORD)
    await flushPromises()

    expect(wrapper.find('[data-test="titlebar"]').text()).toContain('Syncra — Хранилище')
  })

  it('на первом запуске заголовок не врёт про замок', async () => {
    // Запирать нечего: хранилища ещё нет, человек проходит знакомство.
    const { wrapper, router } = await mountWithRouter(App, {
      core: createMockCoreClient({ latencyMs: 0, initialized: false }),
    })

    expect(router.currentRoute.value.name).toBe('setup')
    expect(wrapper.find('[data-test="titlebar"]').text()).toContain('Syncra — знакомство')
  })

  it('переключатель темы один на всё приложение, а не по одному на экран', async () => {
    const { wrapper } = await mountWithRouter(App)

    expect(wrapper.findAll('.sy-theme-toggle')).toHaveLength(1)
    // И живёт он в полосе заголовка: тема — свойство окна, не страницы.
    expect(wrapper.find('[data-test="titlebar"] .sy-theme-toggle').exists()).toBe(true)
  })

  it('автоблокировка сама уводит на экран входа', async () => {
    // До F13 это делал только обработчик кнопки «Заблокировать», поэтому замок
    // по таймауту оставлял на экране метаданные записей до следующего перехода.
    const { router, core } = await mountWithRouter(App)
    expect(router.currentRoute.value.name).toBe('home')

    core.control.forceLock('timeout')
    await flushPromises()

    await waitForRoute(router, 'unlock')
  })
})
