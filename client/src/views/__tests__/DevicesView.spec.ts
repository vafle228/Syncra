import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import {
  createMockCoreClient,
  MOCK_DEVICE_LAPTOP,
  MOCK_RECORD_GITHUB,
  type MockCoreClient,
} from '@/core/mock'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useToastStore } from '@/stores/useToastStore'
import DevicesView from '../DevicesView.vue'

/**
 * Экран сопряжения и доверенных устройств (F8 + F9, §3.6 макета).
 *
 * Проверяем ровно то, ради чего экран существует: код рисуется из ответа ядра,
 * прочитанный код доезжает до ядра, слова-отпечаток сверяет человек — до его
 * подтверждения ничего не сопряжено, — а отзыв доступа спрашивает подтверждения
 * и честно говорит, чего он НЕ делает (§2.3).
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

describe('DevicesView · доверенные устройства и отзыв (F9, §2.3)', () => {
  /** Карточка устройства по его имени: кнопки в списке повторяются. */
  function card(wrapper: View, name: string) {
    const found = wrapper
      .findAll('.trusted__card')
      .find((node) => node.find('.trusted__name').text() === name)
    if (!found) throw new Error(`Карточка устройства «${name}» не найдена`)
    return found
  }

  function cardButton(wrapper: View, name: string, text: string) {
    const found = card(wrapper, name)
      .findAll('button')
      .find((node) => node.text() === text)
    if (!found) throw new Error(`Кнопка «${text}» у устройства «${name}» не найдена`)
    return found
  }

  it('показывает список из ядра и не предлагает отозвать самого себя', async () => {
    const wrapper = await mountDevices()

    const names = wrapper.findAll('.trusted__name').map((node) => node.text())
    expect(names).toContain('Этот компьютер')
    expect(names).toContain('iPhone 14')

    expect(card(wrapper, 'Этот компьютер').text()).toContain('это устройство')
    expect(
      card(wrapper, 'Этот компьютер')
        .findAll('button')
        .some((node) => node.text() === 'Отозвать'),
    ).toBe(false)

    wrapper.unmount()
  })

  it('помечает устройство, которое давно не выходило на связь', async () => {
    const wrapper = await mountDevices()

    expect(card(wrapper, 'ThinkPad X1').text()).toContain('давно не появлялся')
    expect(card(wrapper, 'ThinkPad X1').text()).toContain('не появлялся 41 день')
    expect(card(wrapper, 'iPhone 14').text()).not.toContain('давно не появлялся')

    wrapper.unmount()
  })

  it('«Отозвать» сначала спрашивает и честно говорит, чего отзыв НЕ делает', async () => {
    const wrapper = await mountDevices()

    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать').trigger('click')

    const confirm = card(wrapper, 'ThinkPad X1')
    expect(confirm.text()).toContain('Отозвать доступ у «ThinkPad X1»?')
    // Формулировка из макета: реплику на украденном устройстве отзыв не стирает.
    expect(confirm.text()).toContain('не сможет сопрячься по старому ключу')
    expect(confirm.text()).toContain('остаются в его копии')
    expect(confirm.text()).toContain('смените мастер-пароль')

    // Ничего ещё не произошло: ядро списка не меняло.
    expect((await core.listDevices()).every((device) => device.revoked_at === null)).toBe(true)

    wrapper.unmount()
  })

  it('«Оставить» закрывает подтверждение, ничего не отзывая', async () => {
    const wrapper = await mountDevices()

    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать').trigger('click')
    await cardButton(wrapper, 'ThinkPad X1', 'Оставить').trigger('click')
    await flushPromises()

    expect(wrapper.find('.trusted__confirm').exists()).toBe(false)
    expect((await core.listDevices()).every((device) => device.revoked_at === null)).toBe(true)

    wrapper.unmount()
  })

  it('«Отозвать доступ» доезжает до ядра и оставляет устройство в списке', async () => {
    const wrapper = await mountDevices()

    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать').trigger('click')
    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать доступ').trigger('click')
    await flushPromises()

    const revoked = (await core.listDevices()).find(
      (device) => device.device_id === MOCK_DEVICE_LAPTOP,
    )
    expect(revoked?.revoked_at).not.toBeNull()

    // Отзыв — факт истории: устройство остаётся видимым, но помеченным.
    const still = card(wrapper, 'ThinkPad X1')
    expect(still.text()).toContain('доступ отозван')
    expect(still.text()).toContain('копия на устройстве больше не обновляется')
    expect(wrapper.find('.trusted__confirm').exists()).toBe(false)

    wrapper.unmount()
  })

  it('отказ ядра показывается у того устройства, на котором нажали', async () => {
    const wrapper = await mountDevices()

    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать').trigger('click')
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать доступ').trigger('click')
    await flushPromises()

    expect(card(wrapper, 'ThinkPad X1').text()).toContain('Ядро недоступно.')
    // Список цел: отказ на одном устройстве не гасит остальные.
    expect(card(wrapper, 'iPhone 14').exists()).toBe(true)

    wrapper.unmount()
  })

  it('«Сопрячь заново» ведёт к новому знакомству, а не отменяет отзыв', async () => {
    const wrapper = await mountDevices()

    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать').trigger('click')
    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать доступ').trigger('click')
    await flushPromises()

    // Уводим экран в состояние «сопряжение завершено», чтобы возврат к коду был виден.
    await scan(wrapper)
    await button(wrapper, 'Слова совпадают').trigger('click')
    await flushPromises()
    expect(wrapper.find('.qr__module').exists()).toBe(false)

    await cardButton(wrapper, 'ThinkPad X1', 'Сопрячь заново').trigger('click')
    await flushPromises()

    expect(wrapper.find('.qr__module').exists()).toBe(true)
    // Отзыв остался в силе: вернуть устройство можно только обменом ключами.
    expect(card(wrapper, 'ThinkPad X1').text()).toContain('доступ отозван')

    wrapper.unmount()
  })

  it('сопряжённое устройство появляется в списке сразу', async () => {
    const wrapper = await mountDevices()
    const before = wrapper.findAll('.trusted__card').length

    await scan(wrapper)
    await button(wrapper, 'Слова совпадают').trigger('click')
    await flushPromises()

    expect(wrapper.findAll('.trusted__card')).toHaveLength(before + 1)

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

  it('блокировка убирает и список устройств: он тоже содержимое хранилища', async () => {
    const wrapper = await mountDevices()
    expect(wrapper.findAll('.trusted__card').length).toBeGreaterThan(0)

    core.control.forceLock('timeout')
    await flushPromises()

    expect(wrapper.findAll('.trusted__card')).toHaveLength(0)

    wrapper.unmount()
  })
})

describe('DevicesView · синхронизация (F10)', () => {
  /** Кнопка, которой может и не быть: у «рядом никого» повтора нет намеренно. */
  function maybeButton(wrapper: View, text: string) {
    return wrapper.findAll('button').find((node) => node.text() === text)
  }

  it('объясняет состояние словами и предлагает повтор только после обрыва', async () => {
    const wrapper = await mountDevices()

    // Сид-конфликт заслоняет всё остальное: он единственный ждёт человека.
    expect(wrapper.find('.sync-panel').text()).toContain('в двух местах')
    expect(wrapper.find('.sync-panel__conflicts').exists()).toBe(true)

    await core.resolveConflict(MOCK_RECORD_GITHUB, 'local')
    await useConflictsStore().load()
    await flushPromises()
    // Выбор версии — тоже изменение, и оно ещё не уехало.
    expect(wrapper.find('.sync-panel').text()).toContain('живёт только здесь')

    core.control.finishSync()
    await flushPromises()
    // «Рядом никого» — это не ошибка: ни кнопки «Повторить», ни жёлтого.
    expect(wrapper.find('.sync-panel').text()).toContain('и это нормально')
    expect(maybeButton(wrapper, 'Попробовать сейчас')).toBeUndefined()

    core.control.finishSync('Соединение оборвалось.')
    await flushPromises()
    expect(wrapper.find('.sync-panel').text()).toContain('Данные целы')

    await maybeButton(wrapper, 'Попробовать сейчас')!.trigger('click')
    await flushPromises()
    expect((await core.getSyncStatus()).phase).toBe('searching')

    wrapper.unmount()
  })

  it('сообщает тому, кто показывал код, что второе устройство сопряглось', async () => {
    const wrapper = await mountDevices()
    expect(wrapper.find('.qr__module').exists()).toBe(true)

    core.control.pairedByPeer('Pixel 8')
    await flushPromises()

    // Код своё отработал: держать его на экране значит звать третьего.
    expect(wrapper.find('.qr__module').exists()).toBe(false)
    expect(useToastStore().toasts.some((toast) => toast.text.includes('Pixel 8'))).toBe(true)
    expect(wrapper.findAll('.trusted__name').map((node) => node.text())).toContain('Pixel 8')

    wrapper.unmount()
  })
})
