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
  // Адреса — список: подписи у полей есть, но скрытые («Адрес 1», «Адрес 2»),
  // поэтому ищем по позиции, а не по тексту.
  if (label === 'Адреса сайта') {
    await type(wrapper.find('.form__url-input input'), value)
    return
  }

  // Подпись рисует либо само поле, либо `SyField` — там, где рядом с полем
  // стоит кнопка и подпись обязана быть общей строкой сетки для обоих.
  const field = wrapper.findAll('.sy-input, .sy-field').find((node) => {
    const caption = node.find('.sy-input__label, .sy-field__label')
    return caption.exists() && caption.text() === label
  })
  if (!field) throw new Error(`Поле «${label}» не найдено`)

  await type(field.find('input'), value)
}

function submit(wrapper: Form) {
  return wrapper.find('form').trigger('submit')
}

/** Что показывает свёрнутый список секций. */
function shownVault(wrapper: Form): string {
  return wrapper.find('.form__vault .sy-select__value-text').text()
}

/**
 * Выбрать секцию. Список свой, а не нативный `<select>`, поэтому варианты
 * появляются в DOM только раскрытыми — и искать их можно по тому же, по чему их
 * находит человек: по подписи.
 */
async function pickVault(wrapper: Form, label: string) {
  await wrapper.find('.form__vault .sy-select__trigger').trigger('click')

  const option = wrapper
    .findAll('.form__vault .sy-select__option')
    .find((node) => node.text() === label)
  if (!option) throw new Error(`Секция «${label}» не найдена в списке`)

  await option.trigger('click')
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

  it('в редактировании варианты на виду, но пароль не меняется сам', async () => {
    const { wrapper } = await mountEdit()

    // Панель — это и есть блок «Пароль» из макета: она на месте всегда.
    expect(wrapper.findComponent(PasswordGenerator).exists()).toBe(true)
    expect(wrapper.findAll('.pg__value').length).toBeGreaterThan(0)

    // Но ни один вариант не выбран, поле пустое, и об этом сказано прямо.
    expect(wrapper.find('.pg__variant--picked').exists()).toBe(false)
    expect(passwordInput(wrapper).value).toBe('')
    expect(wrapper.find('.pg__status').text()).toContain(
      'Текущий пароль остаётся, пока не выбран новый',
    )

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

    expect(shownVault(wrapper)).toBe('Личное')

    await wrapper.find('.form__vault .sy-select__trigger').trigger('click')
    expect(wrapper.find('.form__vault').text()).toContain('Рабочее · локальная')

    wrapper.unmount()
  })

  it('предлагает ту секцию, которая открыта в сайдбаре', async () => {
    const list = useRecordsStore()
    await list.load()
    list.setVaultFilter(MOCK_VAULT_WORK)

    const wrapper = mount(RecordForm)
    await flushPromises()

    expect(shownVault(wrapper)).toBe('Рабочее · локальная')
    // О последствиях выбора говорим прямо у поля.
    expect(wrapper.find('.form__vault').text()).toContain('останется на этом устройстве')

    wrapper.unmount()
  })

  it('кладёт новую запись в выбранную секцию', async () => {
    const list = useRecordsStore()
    await list.load()
    const wrapper = mount(RecordForm)
    await flushPromises()

    await pickVault(wrapper, 'Рабочее · локальная')
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

    await pickVault(wrapper, 'Рабочее · локальная')
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

describe('RecordForm · секции в списке и ключ TOTP', () => {
  it('у каждой секции в списке своя цветная метка и ссылка на управление', async () => {
    await useRecordsStore().load()
    const wrapper = mount(RecordForm)
    await flushPromises()

    await wrapper.find('.form__vault .sy-select__trigger').trigger('click')

    const dots = wrapper.findAll('.form__vault .sy-select__option .sy-select__dot')
    expect(dots.length).toBeGreaterThan(0)
    expect(dots[0]!.attributes('style')).toContain('--sy-vault-')
    expect(wrapper.find('.form__vault .sy-select__footer').text()).toBe('Управление секциями…')

    wrapper.unmount()
  })

  it('у новой записи ключ TOTP предлагают завести, а не вводят в пустое поле', async () => {
    await useRecordsStore().load()
    const wrapper = mount(RecordForm)
    await flushPromises()

    expect(wrapper.find('.form__totp--empty').exists()).toBe(true)
    expect(wrapper.find('.form__totp-scan').text()).toBe('Сканировать QR')

    // «Сканировать QR» ведёт в тот же ручной ввод: камеры в MVP 1 нет, и
    // кнопка-пустышка была бы хуже честного пути.
    await wrapper.find('.form__totp-scan').trigger('click')
    expect(wrapper.find('.form__totp--empty').exists()).toBe(false)

    wrapper.unmount()
  })

  it('у записи с ключом форма не показывает сам ключ и даёт его убрать', async () => {
    const { wrapper, list } = await mountEdit()

    expect(wrapper.find('.form__totp--on').text()).toContain('Ключ добавлен')
    expect(wrapper.html()).not.toContain('MOCKTOTPSECRET')

    await wrapper.find('.form__totp-drop').trigger('click')
    await submit(wrapper)
    await flushPromises()

    expect((await core.getSecret(GITHUB)).totp_secret).toBeNull()
    expect(list.records.find((item) => item.record_id === GITHUB)?.has_totp).toBe(false)

    wrapper.unmount()
  })

  it('ключ, который не трогали, остаётся на месте', async () => {
    const { wrapper } = await mountEdit()

    await fill(wrapper, 'Метка аккаунта', 'основной')
    await submit(wrapper)
    await flushPromises()

    expect((await core.getSecret(GITHUB)).totp_secret).toBe('MOCKTOTPSECRET3')

    wrapper.unmount()
  })

  it('заметка показывается закрытой и открывается прямо в поле', async () => {
    const { wrapper } = await mountEdit()

    // Кнопки-соседа больше нет: закрытая заметка сама и есть кнопка.
    expect(wrapper.findAll('button').some((node) => node.text() === 'Показать текущие')).toBe(false)
    expect(wrapper.findAll('.sy-secret__bar')).toHaveLength(2)
    expect(wrapper.find('.form__textarea').exists()).toBe(false)

    await wrapper.find('.sy-secret__box--button').trigger('click')
    await flushPromises()

    expect(wrapper.find<HTMLTextAreaElement>('.form__textarea').element.value).toContain(
      'Recovery codes',
    )

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

/** Форма создания на загруженном списке — как её монтирует `VaultView`. */
async function mountCreate() {
  await useRecordsStore().load()
  const wrapper = mount(RecordForm)
  await flushPromises()
  return wrapper
}

describe('RecordForm · адреса (F13)', () => {
  it('каждое поле подписано, даже когда подпись не видна', async () => {
    // «Текстовое поле» без имени — это то, что услышит человек с экранным
    // диктором. Номер в сетке видит только зрячий.
    const wrapper = await mountCreate()

    const first = wrapper.find('.form__url-row .sy-input__label')
    expect(first.text()).toBe('Адрес 1')
    expect(first.classes()).toContain('sy-input__label--hidden')

    wrapper.unmount()
  })

  it('добавляет и убирает адреса, сохраняя порядок', async () => {
    const wrapper = await mountCreate()

    await wrapper.find('.form__add').trigger('click')
    await wrapper.find('.form__add').trigger('click')
    expect(wrapper.findAll('.form__url-row')).toHaveLength(3)

    const inputs = wrapper.findAll('.form__url-input input')
    await type(inputs[0]!, 'a.example')
    await type(inputs[1]!, 'b.example')
    await type(inputs[2]!, 'c.example')

    // Убираем средний — первый и последний должны остаться на своих местах.
    await wrapper.findAll('.form__url-remove')[1]!.trigger('click')

    const left = wrapper
      .findAll<HTMLInputElement>('.form__url-input input')
      .map((node) => node.element.value)
    expect(left).toEqual(['a.example', 'c.example'])

    wrapper.unmount()
  })

  it('предупреждает про автозаполнение, пока адреса нет', async () => {
    const wrapper = await mountCreate()

    expect(wrapper.find('.form__urls-empty').text()).toContain('автозаполнение не сработает')

    await type(wrapper.find('.form__url-input input'), 'https://figma.com')

    expect(wrapper.find('.form__urls-empty').exists()).toBe(false)

    wrapper.unmount()
  })

  it('объясняет, что станет с адресами в карточке', async () => {
    // Обещание проверяемое: карточка правда показывает первый строкой,
    // остальные чипами (см. `RecordCard.spec`).
    const wrapper = await mountCreate()

    expect(wrapper.find('.form__urls .form__note').text()).toContain('Первый адрес основной')

    wrapper.unmount()
  })
})
