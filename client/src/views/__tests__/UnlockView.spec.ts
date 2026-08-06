import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_MASTER_PASSWORD } from '@/core/mock'
import { useVaultStore } from '@/stores/useVaultStore'
import UnlockView from '../UnlockView.vue'

const push = vi.fn()

vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
}))

beforeEach(() => {
  setActivePinia(createPinia())
  setCoreClient(createMockCoreClient({ latencyMs: 0 }))
  push.mockClear()
})

afterEach(() => {
  setCoreClient(null)
})

async function typeAndSubmit(wrapper: ReturnType<typeof mount>, password: string): Promise<void> {
  await wrapper.find('input').setValue(password)
  await wrapper.find('form').trigger('submit')
  await flushPromises()
}

describe('UnlockView', () => {
  it('открывает хранилище верным паролем и уходит на главный экран', async () => {
    const wrapper = mount(UnlockView)

    await typeAndSubmit(wrapper, MOCK_MASTER_PASSWORD)

    expect(useVaultStore().status).toBe('unlocked')
    expect(push).toHaveBeenCalledWith({ name: 'home' })
  })

  it('показывает сообщение ядра на неверном пароле и никуда не уходит', async () => {
    const wrapper = mount(UnlockView)

    await typeAndSubmit(wrapper, 'не тот пароль')

    expect(wrapper.text()).toContain('Неверный мастер-пароль.')
    expect(push).not.toHaveBeenCalled()
    expect(useVaultStore().isUnlocked).toBe(false)
  })

  it('ЗАКОН №1: введённый пароль не остаётся ни в поле, ни в разметке', async () => {
    const wrapper = mount(UnlockView)
    const password = 'очень-секретная-фраза'

    await typeAndSubmit(wrapper, password)

    expect(wrapper.find('input').element.value).toBe('')
    expect(wrapper.html()).not.toContain(password)
    expect(JSON.stringify(useVaultStore().$state)).not.toContain(password)
  })

  it('поле пароля по умолчанию под маской', () => {
    const wrapper = mount(UnlockView)

    expect(wrapper.find('input').attributes('type')).toBe('password')
  })

  it('объясняет, почему хранилище закрылось само', async () => {
    const vault = useVaultStore()
    await vault.unlock(MOCK_MASTER_PASSWORD)
    vault.lockReason = 'timeout'

    const wrapper = mount(UnlockView)

    expect(wrapper.text()).toContain('не было действий')
  })

  it('не обещает биометрию как рабочую функцию', () => {
    const wrapper = mount(UnlockView)

    expect(wrapper.text()).toContain('Появятся в следующей версии')
  })
})
