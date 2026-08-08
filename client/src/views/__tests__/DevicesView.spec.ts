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
 * Экран доверенных устройств (F9, §2.3 и §3.6 макета).
 *
 * С F13 экран отвечает на один вопрос — кто имеет доступ. Знакомство нового
 * устройства уехало в модалку и проверяется в `PairingModal.spec`; здесь —
 * список, отзыв доступа и панель синхронизации.
 */

let core: MockCoreClient

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
    // Модалку сопряжения не разворачиваем: у неё свой спек.
    global: { stubs: { PairingModal: true } },
  })
  await flushPromises()
  return wrapper
}

type View = Awaited<ReturnType<typeof mountDevices>>

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

  it('«Сопрячь заново» открывает знакомство, а не отменяет отзыв', async () => {
    // Команды «вернуть доверие» нет намеренно: устройство, отрезанное по
    // старому ключу, возвращается только через новое знакомство.
    const wrapper = await mountDevices()

    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать').trigger('click')
    await cardButton(wrapper, 'ThinkPad X1', 'Отозвать доступ').trigger('click')
    await flushPromises()

    expect(wrapper.findComponent({ name: 'PairingModal' }).props('open')).toBe(false)

    await cardButton(wrapper, 'ThinkPad X1', 'Сопрячь заново').trigger('click')
    await flushPromises()

    expect(wrapper.findComponent({ name: 'PairingModal' }).props('open')).toBe(true)
    expect(useToastStore().toasts.some((toast) => toast.text.includes('ThinkPad X1'))).toBe(true)
    // Отзыв остался в силе: вернуть устройство можно только обменом ключами.
    expect(card(wrapper, 'ThinkPad X1').text()).toContain('доступ отозван')

    wrapper.unmount()
  })

  it('«Добавить устройство» открывает знакомство', async () => {
    const wrapper = await mountDevices()

    expect(wrapper.findComponent({ name: 'PairingModal' }).props('open')).toBe(false)

    await wrapper.find('[data-test="pair-open"]').trigger('click')

    expect(wrapper.findComponent({ name: 'PairingModal' }).props('open')).toBe(true)

    wrapper.unmount()
  })
})

describe('DevicesView · Закон №1', () => {
  it('блокировка убирает список устройств: он тоже содержимое хранилища', async () => {
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

  it('устройство, сопряжённое вторым концом, появляется в списке само', async () => {
    // Человек ничего не нажимал: код прочитали на другом устройстве. Список —
    // это ответ ядра, и он должен догнать событие без перезахода на экран.
    const wrapper = await mountDevices()
    expect(wrapper.findAll('.trusted__name').map((node) => node.text())).not.toContain('Pixel 8')

    core.control.pairedByPeer('Pixel 8')
    await flushPromises()

    expect(wrapper.findAll('.trusted__name').map((node) => node.text())).toContain('Pixel 8')

    wrapper.unmount()
  })
})
