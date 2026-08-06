import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount } from '@vue/test-utils'

import SyToast from '@/components/ui/SyToast.vue'
import { TOAST_TIMEOUT_MS, useToastStore } from '../useToastStore'

beforeEach(() => {
  setActivePinia(createPinia())
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('useToastStore', () => {
  it('складывает тосты в очередь и снимает их по таймеру', () => {
    const toasts = useToastStore()

    toasts.push('Пароль скопирован · буфер очистится через 20 с')
    toasts.push('Адрес скопирован')
    expect(toasts.toasts).toHaveLength(2)

    vi.advanceTimersByTime(TOAST_TIMEOUT_MS)
    expect(toasts.toasts).toHaveLength(0)
  })

  it('снимает тост вручную и не будит таймер второй раз', () => {
    const toasts = useToastStore()
    const id = toasts.push('Запись удалена')

    toasts.dismiss(id)
    expect(toasts.toasts).toHaveLength(0)

    vi.advanceTimersByTime(TOAST_TIMEOUT_MS * 2)
    expect(toasts.toasts).toHaveLength(0)
  })

  it('держит тост, если срок не задан', () => {
    const toasts = useToastStore()
    toasts.push('Буфер недоступен', 'danger', 0)

    vi.advanceTimersByTime(TOAST_TIMEOUT_MS * 5)
    expect(toasts.toasts).toHaveLength(1)

    toasts.clear()
    expect(toasts.toasts).toHaveLength(0)
  })
})

describe('SyToast', () => {
  it('рендерит очередь стора и даёт закрыть тост', async () => {
    const toasts = useToastStore()
    const wrapper = mount(SyToast)

    toasts.push('Пароль скопирован · буфер очистится через 20 с')
    await wrapper.vm.$nextTick()

    expect(wrapper.text()).toContain('Пароль скопирован')

    await wrapper.find('.sy-toast__close').trigger('click')
    expect(toasts.toasts).toHaveLength(0)
  })
})
