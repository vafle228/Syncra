import { afterEach, describe, expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient } from '@/core/mock'
import HomeView from '../HomeView.vue'

afterEach(() => {
  setCoreClient(null)
})

describe('HomeView (сквозной путь UI → IPC → мок-ядро)', () => {
  it('рендерит метаданные записей из мок-ядра', async () => {
    setCoreClient(createMockCoreClient({ latencyMs: 0 }))

    const wrapper = mount(HomeView)
    await flushPromises()

    const text = wrapper.text()
    expect(text).toContain('Google')
    expect(text).toContain('personal.demo@gmail.com')
    expect(text).toContain('Рабочий')
  })

  it('не выводит на экран ни одного секрета', async () => {
    setCoreClient(createMockCoreClient({ latencyMs: 0 }))

    const wrapper = mount(HomeView)
    await flushPromises()

    // Все сид-секреты помечены префиксом `mock-` / MOCKTOTP — ничего из этого
    // не должно попасть в разметку списка (Закон №1).
    expect(wrapper.html()).not.toMatch(/mock-[a-z]+-pw/)
    expect(wrapper.html()).not.toContain('MOCKTOTPSECRET')
    expect(wrapper.html()).not.toContain('Recovery codes')
  })

  it('показывает сообщение ядра вместо списка, если ядро вернуло ошибку', async () => {
    const core = createMockCoreClient({ latencyMs: 0 })
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    setCoreClient(core)

    const wrapper = mount(HomeView)
    await flushPromises()

    expect(wrapper.text()).toContain('Ядро недоступно.')
    expect(wrapper.find('ul').exists()).toBe(false)
  })
})
