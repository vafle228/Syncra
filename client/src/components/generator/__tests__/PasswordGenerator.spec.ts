import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, DEFAULT_GENERATOR_PROFILE, type MockCoreClient } from '@/core/mock'
import { useGeneratorStore } from '@/stores/useGeneratorStore'

import GeneratorProfileForm from '../GeneratorProfileForm.vue'
import PasswordGenerator from '../PasswordGenerator.vue'

/**
 * Панель генератора (F6, §3.4 макета) и редактор правил (§3.11).
 */

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

async function mountPanel() {
  const wrapper = mount(PasswordGenerator)
  await flushPromises()
  return wrapper
}

function variants(wrapper: VueWrapper): string[] {
  return wrapper.findAll('.pg__value').map((node) => node.text())
}

function button(wrapper: VueWrapper, label: string) {
  const found = wrapper.findAll('button').find((node) => node.text() === label)
  if (!found) throw new Error(`Кнопка «${label}» не найдена`)
  return found
}

describe('PasswordGenerator', () => {
  it('показывает варианты и оценку стойкости сразу при открытии', async () => {
    const wrapper = await mountPanel()

    expect(variants(wrapper)).toHaveLength(5)
    expect(wrapper.text()).toMatch(/≈ \d+ бит энтропии/)
    expect(wrapper.text()).toContain('случайность берётся у ОС')

    wrapper.unmount()
  })

  it('выбор варианта отдаётся наверх и отмечается на экране', async () => {
    const wrapper = await mountPanel()
    const shown = variants(wrapper)

    await wrapper.findAll('.pg__variant')[2]!.trigger('click')

    expect(wrapper.emitted('pick')).toEqual([[shown[2]]])
    expect(wrapper.findAll('.pg__variant')[2]!.classes()).toContain('pg__variant--picked')
    expect(wrapper.text()).toContain('Выбран')

    wrapper.unmount()
  })

  it('ничего не выбирает само: до нажатия отмеченных вариантов нет', async () => {
    const wrapper = await mountPanel()

    expect(wrapper.find('.pg__variant--picked').exists()).toBe(false)
    expect(wrapper.emitted('pick')).toBeUndefined()

    wrapper.unmount()
  })

  it('«Ещё варианты» приносит новый набор', async () => {
    const wrapper = await mountPanel()
    const before = variants(wrapper)

    await button(wrapper, 'Ещё варианты').trigger('click')
    await flushPromises()

    expect(variants(wrapper)).not.toEqual(before)

    wrapper.unmount()
  })

  it('правила прячутся за кнопкой и раскрываются по ней', async () => {
    const wrapper = await mountPanel()

    expect(wrapper.findComponent(GeneratorProfileForm).exists()).toBe(false)

    await button(wrapper, 'Правила').trigger('click')

    expect(wrapper.findComponent(GeneratorProfileForm).exists()).toBe(true)
    expect(wrapper.text()).toContain('Профиль генерации · настраивается один раз')

    wrapper.unmount()
  })

  it('показывает сообщение ядра, если сгенерировать не удалось', async () => {
    // Правила забираем заранее, чтобы сымитированная ошибка досталась именно
    // генерации, а не загрузке профиля.
    await useGeneratorStore().ensure()
    core.control.failNext('INTERNAL', 'Ядро занято.')

    const wrapper = await mountPanel()

    expect(wrapper.text()).toContain('Ядро занято.')
    expect(variants(wrapper)).toEqual([])

    wrapper.unmount()
  })

  it('«Закрыть» просит родителя убрать панель', async () => {
    const wrapper = await mountPanel()

    await button(wrapper, 'Закрыть').trigger('click')

    expect(wrapper.emitted('close')).toHaveLength(1)

    wrapper.unmount()
  })

  it('ЗАКОН №1: варианты не оседают в Pinia', async () => {
    const store = useGeneratorStore()
    const wrapper = await mountPanel()
    const shown = variants(wrapper)
    expect(shown).toHaveLength(5)

    // Пароли на экране — а в состоянии приложения их нет.
    const snapshot = JSON.stringify(store.$state)
    for (const password of shown) expect(snapshot).not.toContain(password)

    wrapper.unmount()
  })
})

describe('GeneratorProfileForm', () => {
  function mountForm(overrides = {}) {
    return mount(GeneratorProfileForm, {
      props: { modelValue: { ...DEFAULT_GENERATOR_PROFILE }, dirty: false, ...overrides },
    })
  }

  it('показывает ручки режима «символы»', () => {
    const wrapper = mountForm()

    expect(wrapper.text()).toContain('Длина')
    expect(wrapper.text()).toContain('Цифры')
    expect(wrapper.text()).toContain('Без похожих символов')
    expect(wrapper.text()).not.toContain('Сколько слов')

    wrapper.unmount()
  })

  it('переключение режима отдаёт наверх целый профиль', async () => {
    const wrapper = mountForm()

    await button(wrapper, 'слова').trigger('click')

    expect(wrapper.emitted('update:modelValue')).toEqual([
      [{ ...DEFAULT_GENERATOR_PROFILE, mode: 'words' }],
    ])

    wrapper.unmount()
  })

  it('в режиме слов показывает ручки фразы, а не набора символов', () => {
    const wrapper = mountForm({ modelValue: { ...DEFAULT_GENERATOR_PROFILE, mode: 'words' } })

    expect(wrapper.text()).toContain('Сколько слов')
    expect(wrapper.text()).toContain('Разделитель')
    expect(wrapper.text()).toContain('Добавлять число в конец')
    expect(wrapper.text()).not.toContain('Без похожих символов')

    wrapper.unmount()
  })

  it('тумблер объявляет своё состояние вспомогательным технологиям', async () => {
    const wrapper = mountForm()
    const digits = wrapper.findAll('[role="switch"]')[0]!

    expect(digits.attributes('aria-checked')).toBe('true')
    await digits.trigger('click')

    expect(wrapper.emitted('update:modelValue')).toEqual([
      [{ ...DEFAULT_GENERATOR_PROFILE, digits: false }],
    ])

    wrapper.unmount()
  })

  it('ползунок длины ограничен границами ядра', () => {
    const wrapper = mountForm()
    const range = wrapper.find('#gp-length')

    expect(range.attributes('min')).toBe('8')
    expect(range.attributes('max')).toBe('40')

    wrapper.unmount()
  })

  it('сохранять нечего, пока правила не тронули', () => {
    const untouched = mountForm()
    expect(button(untouched, 'Сохранить как профиль').attributes('disabled')).toBeDefined()
    expect(untouched.text()).toContain('Правила сохранены')
    untouched.unmount()

    const edited = mountForm({ dirty: true })
    expect(button(edited, 'Сохранить как профиль').attributes('disabled')).toBeUndefined()
    expect(edited.text()).toContain('Правила изменены')
    edited.unmount()
  })
})
