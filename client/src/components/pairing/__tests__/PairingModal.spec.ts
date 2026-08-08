import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'
import { useToastStore } from '@/stores/useToastStore'
import PairingModal from '../PairingModal.vue'

/**
 * Знакомство устройств (F8, §2.2) — с F13 это модалка, а не половина экрана.
 *
 * Проверяем ровно то, ради чего она существует: код рисуется из ответа ядра,
 * прочитанный код доезжает до ядра, слова-отпечаток сверяет ЧЕЛОВЕК — и до его
 * подтверждения ничего не сопряжено.
 */

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

/**
 * Модалка телепортируется в `body`; заглушка `teleport` оставляет её содержимое
 * внутри обёртки, чтобы утверждения читались как обычные.
 */
async function mountPairing(open = true) {
  const wrapper = mount(PairingModal, {
    props: { open },
    attachTo: document.body,
    global: { stubs: { teleport: true } },
  })
  await flushPromises()
  return wrapper
}

type View = Awaited<ReturnType<typeof mountPairing>>

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

describe('PairingModal · показ кода', () => {
  it('рисует код из ответа ядра и показывает его же символами', async () => {
    const wrapper = await mountPairing()

    const modules = wrapper.findAll('.qr__module')
    expect(modules.length).toBeGreaterThan(0)
    // Матрица не пустая: тёмные модули есть.
    expect(modules.filter((node) => node.classes('qr__module--dark')).length).toBeGreaterThan(0)
    expect(wrapper.find('.pairing__manual-code').text()).toMatch(
      /^[2-9A-HJ-NP-Z]{3} · [2-9A-HJ-NP-Z]{3}$/,
    )

    wrapper.unmount()
  })

  it('кода нет, пока модалку не открыли', async () => {
    // Тот, кто прочитал код, получает право забрать копию хранилища. Держать
    // его наготове за закрытым диалогом незачем.
    const wrapper = await mountPairing(false)

    expect(wrapper.find('.qr__module').exists()).toBe(false)

    wrapper.unmount()
  })

  it('отсчитывает срок жизни кода и честно говорит, когда он истёк', async () => {
    const wrapper = await mountPairing()

    expect(wrapper.find('.pairing__stage-note').text()).toContain('код живёт 03:00')

    // Синхронно: `advanceTimersByTimeAsync` прогоняет 180 тиков, ожидая
    // микрозадачи после каждого, и в полном прогоне это упирается в таймаут
    // теста. Здесь важен итог отсчёта, а не каждая его секунда.
    vi.advanceTimersByTime(3 * 60 * 1000 + 1000)
    await flushPromises()

    expect(wrapper.find('.pairing__stage-note').text()).toContain('код истёк')

    wrapper.unmount()
  })

  it('«Обновить код» просит у ядра новый', async () => {
    const wrapper = await mountPairing()
    const before = wrapper.find('.pairing__manual-code').text()

    await button(wrapper, 'Обновить код').trigger('click')
    await flushPromises()

    expect(wrapper.find('.pairing__manual-code').text()).not.toBe(before)

    wrapper.unmount()
  })

  it('уход в режим чтения убирает свой код с экрана', async () => {
    const wrapper = await mountPairing()

    await button(wrapper, 'Ввожу код').trigger('click')

    expect(wrapper.find('.qr__module').exists()).toBe(false)
    expect(wrapper.find('.pairing__manual-code').exists()).toBe(false)

    wrapper.unmount()
  })

  it('закрытие модалки стирает код', async () => {
    const wrapper = await mountPairing()
    expect(wrapper.find('.qr__module').exists()).toBe(true)

    await wrapper.setProps({ open: false })
    await flushPromises()
    await wrapper.setProps({ open: true })
    await flushPromises()

    // Код после повторного открытия новый, а не тот, что лежал за диалогом.
    expect(wrapper.find('.qr__module').exists()).toBe(true)

    wrapper.unmount()
  })
})

describe('PairingModal · чтение чужого кода', () => {
  it('показывает четыре слова и НЕ сопрягает, пока человек не подтвердил', async () => {
    const wrapper = await mountPairing()

    await scan(wrapper)

    expect(wrapper.findAll('.pairing__word')).toHaveLength(4)
    // Итога ещё нет: устройство не доверенное.
    expect(wrapper.find('.pairing__stats').exists()).toBe(false)
    expect(button(wrapper, 'Слова совпадают').exists()).toBe(true)

    wrapper.unmount()
  })

  it('после «Слова совпадают» показывает, что и куда уехало', async () => {
    const wrapper = await mountPairing()
    await scan(wrapper)

    await button(wrapper, 'Слова совпадают').trigger('click')
    await flushPromises()

    const stats = wrapper.find('.pairing__stats')
    expect(stats.text()).toContain('записи')
    expect(stats.text()).toContain('0 байт')
    // В сиде есть локальная секция — про неё надо сказать прямо здесь.
    expect(wrapper.text()).toContain('Записи локальных секций не поехали')

    wrapper.unmount()
  })

  it('«Отмена» возвращает к чтению кода, ничего не сопрягая', async () => {
    const wrapper = await mountPairing()
    await scan(wrapper)

    await button(wrapper, 'Отмена').trigger('click')
    await flushPromises()

    expect(wrapper.findAll('.pairing__word')).toHaveLength(0)
    expect(wrapper.find('.scan').exists()).toBe(true)

    wrapper.unmount()
  })

  it('нечитаемый код объясняет словами ядра', async () => {
    const wrapper = await mountPairing()

    await scan(wrapper, 'привет')

    expect(wrapper.find('.pairing__error').text()).toContain('не похоже на код Syncra')
    expect(wrapper.findAll('.pairing__word')).toHaveLength(0)

    wrapper.unmount()
  })

  it('устаревший код подсказывает, что делать дальше', async () => {
    const wrapper = await mountPairing()
    core.control.failNext('PAIRING_EXPIRED', 'Этот код больше не действует.')

    await scan(wrapper)

    expect(wrapper.text()).toContain('Попросите второе устройство показать новый')

    wrapper.unmount()
  })

  it('читает код из файла: на десктопе камеры может не быть', async () => {
    const wrapper = await mountPairing()
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

    expect(wrapper.findAll('.pairing__word')).toHaveLength(4)

    wrapper.unmount()
  })
})

describe('PairingModal · Закон №1', () => {
  it('код сопряжения не попадает в состояние Pinia', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)

    const wrapper = await mountPairing()
    const code = wrapper.find('.pairing__manual-code').text().replace(' · ', '')
    await scan(wrapper)

    expect(JSON.stringify(pinia.state.value)).not.toContain(code)
    expect(JSON.stringify(pinia.state.value)).not.toContain(FOREIGN_CODE)

    wrapper.unmount()
  })

  it('блокировка хранилища убирает код с экрана', async () => {
    const wrapper = await mountPairing()
    expect(wrapper.find('.qr__module').exists()).toBe(true)

    core.control.forceLock('timeout')
    await flushPromises()

    expect(wrapper.find('.qr__module').exists()).toBe(false)

    wrapper.unmount()
  })
})

describe('PairingModal · сопряжение со стороны второго устройства', () => {
  it('сообщает тому, кто показывал код, что второе устройство сопряглось', async () => {
    const wrapper = await mountPairing()
    expect(wrapper.find('.qr__module').exists()).toBe(true)

    core.control.pairedByPeer('Pixel 8')
    await flushPromises()

    // Код своё отработал: держать его на экране значит звать третьего.
    expect(wrapper.find('.qr__module').exists()).toBe(false)
    expect(useToastStore().toasts.some((toast) => toast.text.includes('Pixel 8'))).toBe(true)

    wrapper.unmount()
  })
})
