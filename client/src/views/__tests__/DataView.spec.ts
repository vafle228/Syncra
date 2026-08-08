import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_MASTER_PASSWORD, type MockCoreClient } from '@/core/mock'
import { useRecordsStore } from '@/stores/useRecordsStore'
import DataView from '../DataView.vue'

/**
 * Экран данных (F12, §3.10 макета).
 *
 * Проверяем три обещания экрана: предупреждение про CSV показывается и требует
 * подтверждения (§6.2 — формулировку не смягчаем), у опасного файла есть хвост
 * с кнопкой «Удалить файл сейчас», а импорт показывает, что попадёт внутрь, не
 * показывая при этом ни одного пароля.
 */

const push = vi.fn()

vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ query: {} }),
}))

let core: MockCoreClient
let pinia: ReturnType<typeof createPinia>

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
  push.mockClear()
})

afterEach(() => {
  setCoreClient(null)
})

async function mountData() {
  const wrapper = mount(DataView, {
    global: { stubs: { RouterLink: { template: '<a><slot /></a>' } } },
  })
  await flushPromises()
  return wrapper
}

type View = Awaited<ReturnType<typeof mountData>>

function button(wrapper: View, text: string) {
  const found = wrapper.findAll('button').find((node) => node.text().includes(text))
  if (!found) throw new Error(`Кнопка «${text}» не найдена`)
  return found
}

async function openExport(wrapper: View): Promise<void> {
  await button(wrapper, 'Экспорт').trigger('click')
  await flushPromises()
}

/** Заполнить три подтверждения CSV-экспорта и нажать кнопку. */
async function confirmCsv(wrapper: View, password = MOCK_MASTER_PASSWORD): Promise<void> {
  await wrapper.find('.csv__ack-box').setValue(true)
  await wrapper.find('.csv input[type="text"]').setValue('ЭКСПОРТ')
  await wrapper.find('.csv input[type="password"]').setValue(password)
  await button(wrapper, 'в CSV').trigger('click')
  await flushPromises()
}

describe('DataView · CSV-экспорт (§6.2)', () => {
  it('предупреждает об открытом тексте до того, как файл создан', async () => {
    const wrapper = await mountData()
    await openExport(wrapper)

    expect(wrapper.text()).toContain('В файле все пароли и заметки открытым текстом')
    expect(wrapper.text()).toContain('их прочитает любая программа на компьютере')
    expect(wrapper.text()).toContain('TOTP-ключи в CSV не переносятся')

    wrapper.unmount()
  })

  it('не отдаёт файл без всех трёх подтверждений', async () => {
    const wrapper = await mountData()
    await openExport(wrapper)

    expect(button(wrapper, 'в CSV').attributes('disabled')).toBeDefined()

    // Галочка есть, слова нет.
    await wrapper.find('.csv__ack-box').setValue(true)
    expect(button(wrapper, 'в CSV').attributes('disabled')).toBeDefined()

    // Слово есть, мастер-пароля нет.
    await wrapper.find('.csv input[type="text"]').setValue('ЭКСПОРТ')
    expect(button(wrapper, 'в CSV').attributes('disabled')).toBeDefined()

    await wrapper.find('.csv input[type="password"]').setValue(MOCK_MASTER_PASSWORD)
    expect(button(wrapper, 'в CSV').attributes('disabled')).toBeUndefined()

    wrapper.unmount()
  })

  it('не принимает чужой мастер-пароль и файла не создаёт', async () => {
    const wrapper = await mountData()
    await openExport(wrapper)

    await confirmCsv(wrapper, 'не тот пароль')

    expect(wrapper.text()).toContain('Неверный мастер-пароль.')
    expect(wrapper.text()).not.toContain('Открытый файл лежит на диске')

    wrapper.unmount()
  })

  it('после экспорта напоминает про файл и даёт его удалить', async () => {
    const wrapper = await mountData()
    await openExport(wrapper)

    await confirmCsv(wrapper)

    // Формулировку §6.2 не смягчаем: файл не зашифрован, удалите после использования.
    expect(wrapper.text()).toContain('Открытый файл лежит на диске')
    expect(wrapper.text()).toContain('Файл не зашифрован')
    expect(wrapper.text()).toContain('Удалите его сразу после использования')
    expect(wrapper.text()).toContain('syncra-plain-')

    await button(wrapper, 'Удалить файл сейчас').trigger('click')
    await flushPromises()

    expect(wrapper.text()).not.toContain('Открытый файл лежит на диске')
    // Подтверждения одноразовые: следующий такой файл — следующее решение.
    expect(button(wrapper, 'в CSV').attributes('disabled')).toBeDefined()

    wrapper.unmount()
  })
})

describe('DataView · зашифрованный бэкап', () => {
  it('сохраняет файл и показывает, где он лежит', async () => {
    const wrapper = await mountData()
    await openExport(wrapper)

    await wrapper.find('.backup input[type="password"]').setValue(MOCK_MASTER_PASSWORD)
    await button(wrapper, 'Сохранить бэкап').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Бэкап сохранён')
    expect(wrapper.text()).toContain('.syncra')
    expect(wrapper.text()).toContain('Сохранить ещё раз')

    wrapper.unmount()
  })

  it('говорит, что старый бэкап останется на старом пароле', async () => {
    const wrapper = await mountData()
    await openExport(wrapper)

    expect(wrapper.text()).toContain('старый бэкап продолжит открываться старым')

    wrapper.unmount()
  })
})

describe('DataView · импорт', () => {
  it('показывает, что попадёт внутрь, и ни одного пароля', async () => {
    const wrapper = await mountData()

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Файл разобран')
    expect(wrapper.text()).toContain('пароли пока не показываем')
    expect(wrapper.text()).toContain('github.com')
    // Статусы строк видно, значений — нет.
    expect(wrapper.text()).toContain('дубликат')
    expect(wrapper.text()).toContain('без пароля')
    expect(wrapper.html()).not.toContain('mock-import-')

    wrapper.unmount()
  })

  it('переносит записи в отдельную секцию и уводит к ним', async () => {
    const wrapper = await mountData()
    const records = useRecordsStore()
    const before = records.totalAll

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()
    await button(wrapper, 'Импортировать').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('на месте')
    expect(wrapper.text()).toContain('Исходный файл удалён с диска')

    await button(wrapper, 'Показать импортированные').trigger('click')
    await flushPromises()

    expect(records.totalAll).toBeGreaterThan(before)
    expect(records.vaultFilter).not.toBeNull()
    expect(push).toHaveBeenCalledWith({ name: 'home' })

    wrapper.unmount()
  })

  it('отказ от импорта забывает разобранный файл', async () => {
    const wrapper = await mountData()

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()
    await button(wrapper, 'Отмена').trigger('click')
    await flushPromises()

    expect(wrapper.text()).not.toContain('Файл разобран')
    expect(wrapper.find('.import__drop').exists()).toBe(true)

    wrapper.unmount()
  })

  it('молчит, когда окно выбора файла закрыли: это не ошибка', async () => {
    const wrapper = await mountData()
    core.control.cancelNextFilePick()

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()

    expect(wrapper.find('.import__error').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Файл разобран')

    wrapper.unmount()
  })
})

describe('DataView · ЗАКОН №1', () => {
  it('ни путь к открытому файлу, ни разобранный файл не оседают в сторах', async () => {
    const wrapper = await mountData()

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()
    await openExport(wrapper)
    await confirmCsv(wrapper)

    const state = JSON.stringify(pinia.state.value)
    expect(state).not.toContain('syncra-plain-')
    expect(state).not.toContain('session_id')
    expect(state).not.toContain(MOCK_MASTER_PASSWORD)

    wrapper.unmount()
  })

  it('не показывает ни одного секрета хранилища', async () => {
    const wrapper = await mountData()
    await openExport(wrapper)
    await confirmCsv(wrapper)

    expect(wrapper.html()).not.toMatch(/mock-[a-z]+-pw/)
    expect(wrapper.html()).not.toContain('MOCKTOTPSECRET')
    expect(wrapper.html()).not.toContain(MOCK_MASTER_PASSWORD)

    wrapper.unmount()
  })
})
