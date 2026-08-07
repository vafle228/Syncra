import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import PasswordGenerator from '@/components/generator/PasswordGenerator.vue'
import type { RecordMeta } from '@/core/contract'
import { setCoreClient } from '@/core/ipc'
import {
  createMockCoreClient,
  MOCK_VAULT_PERSONAL,
  MOCK_VAULT_WORK,
  type MockCoreClient,
} from '@/core/mock'
import { useGeneratorStore } from '@/stores/useGeneratorStore'
import { useRecordsStore } from '@/stores/useRecordsStore'
import RecordForm from '../RecordForm.vue'

/**
 * Форма записи (F5). Ключевое поведение редактирования: пустое секретное поле
 * означает «не менять», а не «стереть», — и форма не подтягивает секреты сама.
 */

let core: MockCoreClient

const GITHUB = '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1003'

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

type Form = ReturnType<typeof mount>

async function type(input: ReturnType<Form['find']>, value: string) {
  const element = input.element as HTMLInputElement
  element.value = value
  await input.trigger('input')
}

/** Найти поле по подписи и вписать значение. */
async function fill(wrapper: Form, label: string, value: string) {
  // Адреса — список полей без собственных подписей у каждого input.
  if (label === 'Адреса сайта') {
    await type(wrapper.find('.form__url-input input'), value)
    return
  }

  const field = wrapper.findAll('.sy-input').find((node) => {
    const caption = node.find('.sy-input__label')
    return caption.exists() && caption.text() === label
  })
  if (!field) throw new Error(`Поле «${label}» не найдено`)

  await type(field.find('input'), value)
}

function submit(wrapper: Form) {
  return wrapper.find('form').trigger('submit')
}

function passwordInput(wrapper: Form): HTMLInputElement {
  return wrapper.find<HTMLInputElement>('input[type="password"]').element
}

/** Явный разовый reveal ради правки — кнопка рядом с полем пароля. */
async function revealCurrent(wrapper: Form) {
  const button = wrapper.findAll('button').find((node) => node.text() === 'Показать текущий')
  if (!button) throw new Error('Кнопка «Показать текущий» не найдена')

  await button.trigger('click')
  await flushPromises()
}

async function mountEdit() {
  const list = useRecordsStore()
  await list.load()
  const record = list.records.find((item) => item.record_id === GITHUB) as RecordMeta

  const wrapper = mount(RecordForm, { props: { record } })
  await flushPromises()
  return { wrapper, list, record }
}

describe('RecordForm · создание', () => {
  it('создаёт запись в ядре и сообщает о ней наверх', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)

    await fill(wrapper, 'Имя сервиса', 'Figma')
    await fill(wrapper, 'Логин', 'anna@studio.example')
    await fill(wrapper, 'Адреса сайта', 'https://figma.com')
    await fill(wrapper, 'Пароль', 'mock-figma-pw')
    await fill(wrapper, 'Метка аккаунта', 'рабочий')
    await submit(wrapper)
    await flushPromises()

    expect(wrapper.emitted('saved')).toBeTruthy()
    const created = list.records.find((record) => record.service_name === 'Figma')
    expect(created?.login).toBe('anna@studio.example')
    expect(created?.urls).toEqual(['https://figma.com'])
    expect(created?.account_label).toBe('рабочий')
    expect((await core.getSecret(created!.record_id)).password).toBe('mock-figma-pw')

    wrapper.unmount()
  })

  it('не отправляет черновик без обязательных полей', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)

    await fill(wrapper, 'Имя сервиса', 'Figma')
    await submit(wrapper)
    await flushPromises()

    expect(wrapper.emitted('saved')).toBeUndefined()
    expect(wrapper.text()).toContain('Логин обязателен.')
    expect(wrapper.text()).toContain('Пароль обязателен.')
    expect(list.total).toBe(4)

    wrapper.unmount()
  })

  it('не пропускает строку, не похожую на адрес', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)

    await fill(wrapper, 'Имя сервиса', 'Figma')
    await fill(wrapper, 'Логин', 'anna')
    await fill(wrapper, 'Пароль', 'mock-figma-pw')
    await fill(wrapper, 'Адреса сайта', 'htp:/figma')
    await submit(wrapper)
    await flushPromises()

    expect(wrapper.text()).toContain('Не похоже на адрес')
    expect(wrapper.emitted('saved')).toBeUndefined()
    expect(list.total).toBe(4)

    wrapper.unmount()
  })

  it('пустые строки адресов не превращаются в адреса', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)

    await fill(wrapper, 'Имя сервиса', 'Figma')
    await fill(wrapper, 'Логин', 'anna')
    await fill(wrapper, 'Пароль', 'mock-figma-pw')
    await submit(wrapper)
    await flushPromises()

    expect(list.records.find((record) => record.service_name === 'Figma')?.urls).toEqual([])

    wrapper.unmount()
  })

  it('показывает сообщение ядра, если сохранить не удалось', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)
    // Даём панели генератора договорить с ядром: сымитированная ошибка должна
    // достаться сохранению, а не её запросам.
    await flushPromises()
    core.control.failNext('INTERNAL', 'Хранилище занято.')

    await fill(wrapper, 'Имя сервиса', 'Figma')
    await fill(wrapper, 'Логин', 'anna')
    await fill(wrapper, 'Пароль', 'mock-figma-pw')
    await submit(wrapper)
    await flushPromises()

    expect(wrapper.text()).toContain('Хранилище занято.')
    expect(wrapper.emitted('saved')).toBeUndefined()

    wrapper.unmount()
  })
})

describe('RecordForm · генератор (F6)', () => {
  it('в новой записи предлагает готовые варианты сразу', async () => {
    await useRecordsStore().load()
    const wrapper = mount(RecordForm)
    await flushPromises()

    // §6.1: правила настроены один раз — в форме уже лежит готовый пароль.
    expect(wrapper.findComponent(PasswordGenerator).exists()).toBe(true)
    expect(wrapper.findAll('.pg__value').length).toBeGreaterThan(0)
    // Но поле пароля пустое: ни один вариант не подставился сам.
    expect(passwordInput(wrapper).value).toBe('')

    wrapper.unmount()
  })

  it('выбранный вариант попадает в поле пароля и сохраняется в ядро', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)
    await flushPromises()

    const chosen = wrapper.findAll('.pg__value')[1]!.text()
    await wrapper.findAll('.pg__variant')[1]!.trigger('click')

    expect(passwordInput(wrapper).value).toBe(chosen)

    await fill(wrapper, 'Имя сервиса', 'Figma')
    await fill(wrapper, 'Логин', 'anna@studio.example')
    await submit(wrapper)
    await flushPromises()

    const created = list.records.find((record) => record.service_name === 'Figma')
    expect((await core.getSecret(created!.record_id)).password).toBe(chosen)

    wrapper.unmount()
  })

  it('не мешает вписать свой пароль руками', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)
    await flushPromises()

    await fill(wrapper, 'Имя сервиса', 'Figma')
    await fill(wrapper, 'Логин', 'anna')
    await fill(wrapper, 'Пароль', 'свой-собственный-пароль')
    await submit(wrapper)
    await flushPromises()

    const created = list.records.find((record) => record.service_name === 'Figma')
    expect((await core.getSecret(created!.record_id)).password).toBe('свой-собственный-пароль')

    wrapper.unmount()
  })

  it('в редактировании панель закрыта: замена пароля — осознанное действие', async () => {
    const { wrapper } = await mountEdit()

    expect(wrapper.findComponent(PasswordGenerator).exists()).toBe(false)

    const open = wrapper.findAll('button').find((node) => node.text() === 'Подобрать')
    await open!.trigger('click')
    await flushPromises()

    expect(wrapper.findComponent(PasswordGenerator).exists()).toBe(true)

    wrapper.unmount()
  })

  it('ЗАКОН №1: варианты не оседают в Pinia', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)
    await flushPromises()

    const shown = wrapper.findAll('.pg__value').map((node) => node.text())
    expect(shown.length).toBeGreaterThan(0)

    const snapshot = JSON.stringify(list.$state) + JSON.stringify(useGeneratorStore().$state)
    for (const password of shown) expect(snapshot).not.toContain(password)

    wrapper.unmount()
  })
})

describe('RecordForm · секция (F7)', () => {
  it('по умолчанию предлагает секцию по умолчанию из ядра', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)
    await flushPromises()

    const select = wrapper.find<HTMLSelectElement>('.form__vault select')
    expect(select.element.value).toBe(MOCK_VAULT_PERSONAL)
    expect(wrapper.find('.form__vault').text()).toContain('Рабочее · локальная')

    wrapper.unmount()
  })

  it('предлагает ту секцию, которая открыта в сайдбаре', async () => {
    const list = useRecordsStore()
    await list.load()
    list.setVaultFilter(MOCK_VAULT_WORK)

    const wrapper = mount(RecordForm)
    await flushPromises()

    expect(wrapper.find<HTMLSelectElement>('.form__vault select').element.value).toBe(
      MOCK_VAULT_WORK,
    )
    // О последствиях выбора говорим прямо у поля.
    expect(wrapper.find('.form__vault').text()).toContain('останется на этом устройстве')

    wrapper.unmount()
  })

  it('кладёт новую запись в выбранную секцию', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)
    await flushPromises()

    await wrapper.find('.form__vault select').setValue(MOCK_VAULT_WORK)
    await fill(wrapper, 'Имя сервиса', 'Figma')
    await fill(wrapper, 'Логин', 'anna')
    await fill(wrapper, 'Пароль', 'mock-figma-pw')
    await submit(wrapper)
    await flushPromises()

    expect(list.records.find((record) => record.service_name === 'Figma')?.vault_id).toBe(
      MOCK_VAULT_WORK,
    )

    wrapper.unmount()
  })

  it('переносит запись в другую секцию при сохранении', async () => {
    const { wrapper, list, record } = await mountEdit()
    expect(record.vault_id).toBe(MOCK_VAULT_PERSONAL)

    await wrapper.find('.form__vault select').setValue(MOCK_VAULT_WORK)
    await submit(wrapper)
    await flushPromises()

    expect(list.records.find((item) => item.record_id === GITHUB)?.vault_id).toBe(MOCK_VAULT_WORK)

    wrapper.unmount()
  })

  it('не двигает секцию, если её не трогали', async () => {
    const { wrapper, list, record } = await mountEdit()

    await fill(wrapper, 'Метка аккаунта', 'основной')
    await submit(wrapper)
    await flushPromises()

    expect(list.records.find((item) => item.record_id === GITHUB)?.vault_id).toBe(record.vault_id)

    wrapper.unmount()
  })
})

describe('RecordForm · редактирование (ЗАКОН №1)', () => {
  it('открывается с метаданными, но без единого секрета', async () => {
    const { wrapper } = await mountEdit()

    expect(wrapper.find<HTMLInputElement>('.sy-input input').element.value).toBe('GitHub')
    expect(wrapper.html()).not.toContain('mock-github-pw')
    expect(wrapper.html()).not.toContain('MOCKTOTPSECRET')
    expect(wrapper.html()).not.toContain('Recovery codes')

    wrapper.unmount()
  })

  it('пустое секретное поле означает «оставить как было»', async () => {
    const { wrapper, record } = await mountEdit()

    await fill(wrapper, 'Метка аккаунта', 'основной')
    await submit(wrapper)
    await flushPromises()

    const updated = useRecordsStore().records.find((item) => item.record_id === GITHUB)
    expect(updated?.account_label).toBe('основной')
    // Пароль не трогали: ни значение, ни дата его смены не изменились.
    expect((await core.getSecret(GITHUB)).password).toBe('mock-github-pw')
    expect(updated?.password_updated_at).toBe(record.password_updated_at)
    expect(updated?.has_notes).toBe(true)

    wrapper.unmount()
  })

  it('заменяет пароль, когда его действительно ввели', async () => {
    const { wrapper, record } = await mountEdit()

    await fill(wrapper, 'Пароль', 'mock-github-pw-2')
    await submit(wrapper)
    await flushPromises()

    expect((await core.getSecret(GITHUB)).password).toBe('mock-github-pw-2')
    const updated = useRecordsStore().records.find((item) => item.record_id === GITHUB)
    expect(updated?.password_updated_at).not.toBe(record.password_updated_at)

    wrapper.unmount()
  })

  it('подставляет текущее значение только по явному нажатию', async () => {
    const { wrapper } = await mountEdit()

    await revealCurrent(wrapper)

    expect(wrapper.find<HTMLInputElement>('input[type="password"]').element.value).toBe(
      'mock-github-pw',
    )

    wrapper.unmount()
  })

  it('не даёт стереть пароль в ноль', async () => {
    const { wrapper } = await mountEdit()

    await revealCurrent(wrapper)

    await fill(wrapper, 'Пароль', '')
    await submit(wrapper)
    await flushPromises()

    expect(wrapper.text()).toContain('Пароль не может быть пустым.')
    expect((await core.getSecret(GITHUB)).password).toBe('mock-github-pw')

    wrapper.unmount()
  })

  it('черновик с секретом не оседает в Pinia', async () => {
    const { wrapper, list } = await mountEdit()

    await fill(wrapper, 'Пароль', 'mock-github-pw-3')
    await submit(wrapper)
    await flushPromises()

    expect(JSON.stringify(list.$state)).not.toContain('mock-github-pw-3')

    wrapper.unmount()
  })
})
