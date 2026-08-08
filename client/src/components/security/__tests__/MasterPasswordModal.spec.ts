import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_MASTER_PASSWORD, type MockCoreClient } from '@/core/mock'
import { useToastStore } from '@/stores/useToastStore'
import MasterPasswordModal from '../MasterPasswordModal.vue'

/**
 * Смена мастер-пароля (F13).
 *
 * Главное, что здесь проверяется, — поле подтверждения, которого в прототипе
 * нет: пути восстановления у продукта не существует, и опечатка в новом пароле
 * означала бы потерю всего хранилища.
 */

let core: MockCoreClient

const NEW_PASSWORD = 'новый длинный пароль'

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

async function mountModal(open = true) {
  const wrapper = mount(MasterPasswordModal, {
    props: { open },
    attachTo: document.body,
    global: { stubs: { teleport: true } },
  })
  await flushPromises()
  return wrapper
}

type View = Awaited<ReturnType<typeof mountModal>>

/** Заполнить поле по видимой подписи. */
async function fill(wrapper: View, label: string, value: string) {
  const field = wrapper.findAll('.sy-input').find((node) => {
    const caption = node.find('.sy-input__label')
    return caption.exists() && caption.text() === label
  })
  if (!field) throw new Error(`Поле «${label}» не найдено`)

  const input = field.find('input')
  ;(input.element as HTMLInputElement).value = value
  await input.trigger('input')
}

const submit = (wrapper: View) => wrapper.find('[data-test="master-password-submit"]')

describe('MasterPasswordModal · подтверждение нового пароля', () => {
  it('не даёт сменить пароль, пока подтверждение не совпало', async () => {
    const wrapper = await mountModal()

    await fill(wrapper, 'Текущий мастер-пароль', MOCK_MASTER_PASSWORD)
    await fill(wrapper, 'Новый мастер-пароль', NEW_PASSWORD)
    await fill(wrapper, 'Новый пароль ещё раз', 'новый длинный парол')

    expect(wrapper.text()).toContain('Пароли не совпадают.')
    expect(submit(wrapper).attributes('disabled')).toBeDefined()

    wrapper.unmount()
  })

  it('не даёт сохранить пароль короче порога ядра', async () => {
    const wrapper = await mountModal()

    await fill(wrapper, 'Текущий мастер-пароль', MOCK_MASTER_PASSWORD)
    await fill(wrapper, 'Новый мастер-пароль', 'корот')

    // Порог берётся из контракта, а не из «не короче 12» в макете.
    expect(wrapper.text()).toContain('Не короче 8 символов.')
    expect(submit(wrapper).attributes('disabled')).toBeDefined()

    wrapper.unmount()
  })

  it('не принимает тот же самый пароль', async () => {
    const wrapper = await mountModal()

    await fill(wrapper, 'Текущий мастер-пароль', MOCK_MASTER_PASSWORD)
    await fill(wrapper, 'Новый мастер-пароль', MOCK_MASTER_PASSWORD)
    await fill(wrapper, 'Новый пароль ещё раз', MOCK_MASTER_PASSWORD)

    expect(wrapper.text()).toContain('Это тот же пароль, что и сейчас.')
    expect(submit(wrapper).attributes('disabled')).toBeDefined()

    wrapper.unmount()
  })
})

describe('MasterPasswordModal · смена', () => {
  it('доезжает до ядра и оставляет хранилище открытым', async () => {
    const wrapper = await mountModal()

    await fill(wrapper, 'Текущий мастер-пароль', MOCK_MASTER_PASSWORD)
    await fill(wrapper, 'Новый мастер-пароль', NEW_PASSWORD)
    await fill(wrapper, 'Новый пароль ещё раз', NEW_PASSWORD)
    await submit(wrapper).trigger('click')
    await flushPromises()

    // Старый пароль ядро больше не принимает, новый — принимает.
    await expect(core.exportCsv(MOCK_MASTER_PASSWORD)).rejects.toThrow()
    await expect(core.exportCsv(NEW_PASSWORD)).resolves.toBeDefined()

    // Запирать за подтверждённый пароль — наказывать за успех.
    expect(core.control.isUnlocked()).toBe(true)
    expect(wrapper.emitted('close')).toBeTruthy()
    expect(useToastStore().toasts.some((t) => t.text.includes('Мастер-пароль изменён'))).toBe(true)

    wrapper.unmount()
  })

  it('показывает отказ ядра словами ядра', async () => {
    const wrapper = await mountModal()

    await fill(wrapper, 'Текущий мастер-пароль', 'не тот пароль')
    await fill(wrapper, 'Новый мастер-пароль', NEW_PASSWORD)
    await fill(wrapper, 'Новый пароль ещё раз', NEW_PASSWORD)
    await submit(wrapper).trigger('click')
    await flushPromises()

    expect(wrapper.find('.master__error').exists()).toBe(true)
    expect(wrapper.emitted('close')).toBeFalsy()

    wrapper.unmount()
  })
})

describe('MasterPasswordModal · ЗАКОН №1', () => {
  it('пароли не оседают ни в Pinia, ни в разметке после закрытия', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)

    const wrapper = await mountModal()
    await fill(wrapper, 'Текущий мастер-пароль', MOCK_MASTER_PASSWORD)
    await fill(wrapper, 'Новый мастер-пароль', NEW_PASSWORD)

    expect(JSON.stringify(pinia.state.value)).not.toContain(NEW_PASSWORD)
    expect(JSON.stringify(pinia.state.value)).not.toContain(MOCK_MASTER_PASSWORD)

    // Закрыли — черновики не переживают диалог.
    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    await flushPromises()

    const values = wrapper.findAll('input').map((node) => (node.element as HTMLInputElement).value)
    expect(values.every((value) => value === '')).toBe(true)

    wrapper.unmount()
  })
})
