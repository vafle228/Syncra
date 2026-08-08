import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_MASTER_PASSWORD, type MockCoreClient } from '@/core/mock'
import { useVaultStore } from '@/stores/useVaultStore'
import SetupView from '../SetupView.vue'

const push = vi.fn()

vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
}))

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, initialized: false })
  setCoreClient(core)
  push.mockClear()
})

afterEach(() => {
  setCoreClient(null)
})

/** Со вступительного шага — к вводу пароля. */
async function goToPasswordStep(wrapper: ReturnType<typeof mount>): Promise<void> {
  await wrapper.find('.setup__actions button').trigger('click')
}

async function createVault(wrapper: ReturnType<typeof mount>, password: string): Promise<void> {
  await goToPasswordStep(wrapper)
  await wrapper.find('input').setValue(password)
  await wrapper.find('.setup__actions button').trigger('click')
  await flushPromises()
}

describe('SetupView', () => {
  it('начинает с объяснения модели «нет сервера»', () => {
    const wrapper = mount(SetupView)

    expect(wrapper.text()).toContain('Здесь не нужно заводить аккаунт')
    expect(wrapper.text()).toContain('Регистрироваться негде')
  })

  it('предупреждает, что восстановления не будет — формулировку не смягчаем', async () => {
    const wrapper = mount(SetupView)
    await goToPasswordStep(wrapper)

    expect(wrapper.text()).toContain('Восстановления не будет')
    expect(wrapper.text()).toContain('почты и сервера в этой схеме нет')
  })

  it('не даёт создать хранилище коротким паролем — кнопка выключена', async () => {
    const wrapper = mount(SetupView)
    await goToPasswordStep(wrapper)
    await wrapper.find('input').setValue('корот')

    expect(wrapper.find('.setup__actions button').attributes('disabled')).toBeDefined()
    expect(core.control.isInitialized()).toBe(false)
  })

  it('создаёт хранилище и переводит его в разблокированное состояние', async () => {
    const wrapper = mount(SetupView)

    await createVault(wrapper, 'рыжий трамвай у моста')

    expect(core.control.isInitialized()).toBe(true)
    expect(useVaultStore().status).toBe('unlocked')
    expect(wrapper.text()).toContain('Хранилище создано')
  })

  it('ЗАКОН №1: заданный мастер-пароль не остаётся ни в поле, ни в сторе', async () => {
    const wrapper = mount(SetupView)
    const password = 'рыжий трамвай у моста'

    await createVault(wrapper, password)

    expect(wrapper.html()).not.toContain(password)
    expect(JSON.stringify(useVaultStore().$state)).not.toContain(password)
  })

  it('на ошибке ядра остаётся на шаге пароля и показывает сообщение', async () => {
    core.control.failNext('INTERNAL', 'Не удалось создать файл хранилища.')
    const wrapper = mount(SetupView)

    await createVault(wrapper, 'рыжий трамвай у моста')

    expect(wrapper.text()).toContain('Не удалось создать файл хранилища.')
    expect(wrapper.text()).not.toContain('Хранилище создано')
  })

  it('с последнего шага уводит в приложение', async () => {
    const wrapper = mount(SetupView)
    await createVault(wrapper, 'рыжий трамвай у моста')

    await wrapper.find('.setup__actions button').trigger('click')

    expect(push).toHaveBeenCalledWith({ name: 'home' })
  })
})

/**
 * Восстановление из зашифрованного бэкапа (F12, §6.3). Живёт здесь, а не в
 * настройках: бэкап нужен, когда устройств не осталось, то есть на чистой машине.
 */
describe('SetupView · восстановление из бэкапа', () => {
  async function goToRestore(wrapper: ReturnType<typeof mount>): Promise<void> {
    const enter = wrapper.findAll('button').find((node) => node.text() === 'Восстановить из бэкапа')
    await enter!.trigger('click')
  }

  async function restore(wrapper: ReturnType<typeof mount>, password: string): Promise<void> {
    await goToRestore(wrapper)
    await wrapper.find('input').setValue(password)
    await wrapper.find('.setup__actions button').trigger('click')
    await flushPromises()
  }

  it('объясняет, чем бэкап отличается от синхронизации', async () => {
    const wrapper = mount(SetupView)
    await goToRestore(wrapper)

    expect(wrapper.text()).toContain('Бэкап — не синхронизация')
    expect(wrapper.text()).toContain('осталось на потерянных устройствах')
  })

  it('открывает хранилище паролем от файла и показывает, что приехало', async () => {
    const wrapper = mount(SetupView)

    await restore(wrapper, MOCK_MASTER_PASSWORD)

    expect(core.control.isInitialized()).toBe(true)
    expect(useVaultStore().status).toBe('unlocked')
    expect(wrapper.text()).toContain('Хранилище восстановлено')
    expect(wrapper.text()).toContain('.syncra')
  })

  it('не принимает чужой пароль и хранилища не создаёт', async () => {
    const wrapper = mount(SetupView)

    await restore(wrapper, 'чужой пароль')

    expect(wrapper.text()).toContain('Этот бэкап закрыт другим мастер-паролем.')
    expect(core.control.isInitialized()).toBe(false)
  })

  it('молчит, когда окно выбора файла закрыли', async () => {
    const wrapper = mount(SetupView)
    core.control.cancelNextFilePick()

    await restore(wrapper, MOCK_MASTER_PASSWORD)

    expect(wrapper.text()).toContain('Файл не выбран')
    expect(core.control.isInitialized()).toBe(false)
  })

  it('ЗАКОН №1: пароль от бэкапа не остаётся ни в поле, ни в сторе', async () => {
    const wrapper = mount(SetupView)

    await restore(wrapper, MOCK_MASTER_PASSWORD)

    expect(wrapper.html()).not.toContain(MOCK_MASTER_PASSWORD)
    expect(JSON.stringify(useVaultStore().$state)).not.toContain(MOCK_MASTER_PASSWORD)
  })
})
