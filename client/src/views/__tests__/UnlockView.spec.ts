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

  it('не обещает быстрый вход как что-то, что уезжает с устройства', async () => {
    const wrapper = mount(UnlockView)
    await flushPromises()

    // PIN отпирает локальное представление ключа на ЭТОМ устройстве и не
    // синхронизируется — обещать иное значило бы обещать второй мастер-пароль.
    expect(wrapper.text()).toContain('не замена ему')
    expect(wrapper.text()).toContain('с него же не уезжают')
  })

  it('обещание продукта стоит у нижнего края окна, а не в форме', async () => {
    const wrapper = mount(UnlockView)
    await flushPromises()

    expect(wrapper.find('.unlock__ground').text()).toContain('сервера нет')
  })
})

describe('UnlockView · быстрый вход по PIN (F13)', () => {
  /** Хранилище с включённым быстрым входом: решает ЯДРО, а не экран. */
  async function mountWithPin(pin = '2468') {
    const core = createMockCoreClient({ latencyMs: 0, pin })
    setCoreClient(core)
    const wrapper = mount(UnlockView)
    await flushPromises()
    return { wrapper, core }
  }

  async function tap(wrapper: ReturnType<typeof mount>, digits: string) {
    for (const digit of digits) {
      const key = wrapper.findAll('.unlock__key').find((node) => node.text() === digit)
      if (!key) throw new Error(`Клавиша «${digit}» не найдена`)
      await key.trigger('click')
    }
    await flushPromises()
  }

  it('клавиатуры нет, пока ядро не подтвердило быстрый вход', async () => {
    // Рисовать клавиатуру, которая ничего не отпирает, — обман.
    const wrapper = mount(UnlockView)
    await flushPromises()

    expect(wrapper.find('[data-test="pin-pad"]').exists()).toBe(false)
    expect(wrapper.find('input[type="password"]').exists()).toBe(true)
  })

  it('верный PIN открывает хранилище', async () => {
    const { wrapper } = await mountWithPin()

    expect(wrapper.find('[data-test="pin-pad"]').exists()).toBe(true)

    await tap(wrapper, '2468')

    expect(useVaultStore().status).toBe('unlocked')
    expect(push).toHaveBeenCalledWith({ name: 'home' })
  })

  it('неверный PIN — это опечатка, а не сбой: счётчик попыток на виду', async () => {
    const { wrapper } = await mountWithPin()

    await tap(wrapper, '1111')

    expect(wrapper.find('.unlock__pin-note').text()).toContain('Осталось попыток')
    expect(useVaultStore().isUnlocked).toBe(false)
    // Набранное стёрлось: следующая попытка начинается с чистого поля.
    expect(wrapper.findAll('.unlock__cell--filled')).toHaveLength(0)
  })

  it('исчерпанные попытки выключают быстрый вход и уводят на мастер-пароль', async () => {
    const { wrapper } = await mountWithPin()

    for (let attempt = 0; attempt < 5; attempt += 1) await tap(wrapper, '1111')

    expect(wrapper.find('[data-test="pin-pad"]').exists()).toBe(false)
    // Объяснение — то, что дал сам обработчик: он знает, что именно случилось.
    expect(wrapper.text()).toContain('Попытки кончились')
    expect(wrapper.find('input[type="password"]').exists()).toBe(true)
  })

  it('мастер-пароль доступен и при включённом PIN', async () => {
    // PIN — не замена ключу: настоящий вход должен быть в одном нажатии.
    const { wrapper } = await mountWithPin()

    const toMaster = wrapper.findAll('button').find((n) => n.text() === 'Ввести мастер-пароль')!
    await toMaster.trigger('click')

    expect(wrapper.find('input[type="password"]').exists()).toBe(true)

    await typeAndSubmit(wrapper, MOCK_MASTER_PASSWORD)
    expect(useVaultStore().status).toBe('unlocked')
  })

  it('ЗАКОН №1: набранный PIN не оседает в состоянии', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const { wrapper } = await mountWithPin()

    await tap(wrapper, '246')
    expect(JSON.stringify(pinia.state.value)).not.toContain('246')

    await tap(wrapper, '8')
    expect(JSON.stringify(pinia.state.value)).not.toContain('2468')
  })
})
