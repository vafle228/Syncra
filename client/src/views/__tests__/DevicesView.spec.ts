import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'
import DevicesView from '../DevicesView.vue'

/**
 * Экран сопряжения (F8, §3.6 макета).
 *
 * Проверяем ровно то, ради чего экран существует: код рисуется из ответа ядра,
 * прочитанный код доезжает до ядра, а слова-отпечаток сверяет человек — до его
 * подтверждения ничего не сопряжено.
 */

const push = vi.fn()

vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
}))

let core: MockCoreClient

/** Заведомо чужой код: своим же кодом сопрячься нельзя, и это правильно. */
const FOREIGN_CODE = '4TQ9MB'

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-08-08T10:00:00.000Z'))
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
  vi.useRealTimers()
})

async function mountDevices() {
  const wrapper = mount(DevicesView, {
    attachTo: document.body,
    global: { stubs: { RouterLink: true } },
  })
  await flushPromises()
  return wrapper
}

type View = Awaited<ReturnType<typeof mountDevices>>

function button(wrapper: View, text: string) {
  const found = wrapper.findAll('button').find((node) => node.text() === text)
  if (!found) throw new Error(`Кнопка «${text}» не найдена`)
  return found
}

/** Пройти путь «прочитал код» до экрана сверки слов. */
async function scan(wrapper: View, code = FOREIGN_CODE): Promise<void> {
  await button(wrapper, 'Ввожу код').trigger('click')
  await wrapper.find('.scan input').setValue(code)
  await button(wrapper, 'Прочитать код').trigger('click')
  await flushPromises()
}

describe('DevicesView · показ кода', () => {
  it('рисует код из ответа ядра и показывает его же символами', async () => {
    const wrapper = await mountDevices()

    const modules = wrapper.findAll('.qr__module')
    expect(modules.length).toBeGreaterThan(0)
    // Матрица не пустая: тёмные модули есть.
    expect(modules.filter((node) => node.classes('qr__module--dark')).length).toBeGreaterThan(0)
    expect(wrapper.find('.devices__manual-code').text()).toMatch(
      /^[2-9A-HJ-NP-Z]{3} · [2-9A-HJ-NP-Z]{3}$/,
    )

    wrapper.unmount()
  })

  it('отсчитывает срок жизни кода и честно говорит, когда он истёк', async () => {
    const wrapper = await mountDevices()

    expect(wrapper.find('.devices__stage-note').text()).toContain('код живёт 03:00')

    await vi.advanceTimersByTimeAsync(3 * 60 * 1000 + 1000)
    expect(wrapper.find('.devices__stage-note').text()).toContain('код истёк')

    wrapper.unmount()
  })

  it('«Обновить код» просит у ядра новый', async () => {
    const wrapper = await mountDevices()
    const before = wrapper.find('.devices__manual-code').text()

    await button(wrapper, 'Обновить код').trigger('click')
    await flushPromises()

    expect(wrapper.find('.devices__manual-code').text()).not.toBe(before)

    wrapper.unmount()
  })

  it('уход в режим чтения убирает свой код с экрана', async () => {
    const wrapper = await mountDevices()

    await button(wrapper, 'Ввожу код').trigger('click')

    expect(wrapper.find('.qr__module').exists()).toBe(false)
    expect(wrapper.find('.devices__manual-code').exists()).toBe(false)

    wrapper.unmount()
  })
})

describe('DevicesView · чтение чужого кода', () => {
  it('показывает четыре слова и НЕ сопрягает, пока человек не подтвердил', async () => {
    const wrapper = await mountDevices()

    await scan(wrapper)

    expect(wrapper.findAll('.devices__word')).toHaveLength(4)
    // Итога ещё нет: устройство не доверенное.
    expect(wrapper.find('.devices__stats').exists()).toBe(false)
    expect(button(wrapper, 'Слова совпадают').exists()).toBe(true)

    wrapper.unmount()
  })

  it('после «Слова совпадают» показывает, что и куда уехало', async () => {
    const wrapper = await mountDevices()
    await scan(wrapper)

    await button(wrapper, 'Слова совпадают').trigger('click')
    await flushPromises()

    const stats = wrapper.find('.devices__stats')
    expect(stats.text()).toContain('записи')
    expect(stats.text()).toContain('0 байт')
    // В сиде есть локальная секция — про неё надо сказать прямо здесь.
    expect(wrapper.text()).toContain('Записи локальных секций не поехали')

    wrapper.unmount()
  })

  it('«Отмена» возвращает к чтению кода, ничего не сопрягая', async () => {
    const wrapper = await mountDevices()
    await scan(wrapper)

    await button(wrapper, 'Отмена').trigger('click')
    await flushPromises()

    expect(wrapper.findAll('.devices__word')).toHaveLength(0)
    expect(wrapper.find('.scan').exists()).toBe(true)

    wrapper.unmount()
  })

  it('нечитаемый код объясняет словами ядра', async () => {
    const wrapper = await mountDevices()

    await scan(wrapper, 'привет')

    expect(wrapper.find('.devices__error').text()).toContain('не похоже на код Syncra')
    expect(wrapper.findAll('.devices__word')).toHaveLength(0)

    wrapper.unmount()
  })

  it('устаревший код подсказывает, что делать дальше', async () => {
    const wrapper = await mountDevices()
    core.control.failNext('PAIRING_EXPIRED', 'Этот код больше не действует.')

    await scan(wrapper)

    expect(wrapper.text()).toContain('Попросите второе устройство показать новый')

    wrapper.unmount()
  })

  it('читает код из файла: на десктопе камеры может не быть', async () => {
    const wrapper = await mountDevices()
    await button(wrapper, 'Ввожу код').trigger('click')

    const input = wrapper.find('.scan__file')
    const file = new File([`syncra-pair:4tq9mb.${'ab'.repeat(16)}`], 'pair.txt', {
      type: 'text/plain',
    })
    Object.defineProperty(input.element, 'files', { value: [file], configurable: true })
    await input.trigger('change')
    // `FileReader` отвечает не в микрозадаче — ждём его отдельно.
    await vi.runAllTimersAsync()
    await flushPromises()

    expect(wrapper.findAll('.devices__word')).toHaveLength(4)

    wrapper.unmount()
  })
})

describe('DevicesView · Закон №1', () => {
  it('код сопряжения не попадает в состояние Pinia', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)

    const wrapper = await mountDevices()
    const code = wrapper.find('.devices__manual-code').text().replace(' · ', '')
    await scan(wrapper)

    expect(JSON.stringify(pinia.state.value)).not.toContain(code)
    expect(JSON.stringify(pinia.state.value)).not.toContain(FOREIGN_CODE)

    wrapper.unmount()
  })

  it('блокировка хранилища убирает код с экрана', async () => {
    const wrapper = await mountDevices()
    expect(wrapper.find('.qr__module').exists()).toBe(true)

    core.control.forceLock('timeout')
    await flushPromises()

    expect(wrapper.find('.qr__module').exists()).toBe(false)

    wrapper.unmount()
  })
})
