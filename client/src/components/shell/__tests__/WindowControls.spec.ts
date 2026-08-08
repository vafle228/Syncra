import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import { windowControlsAvailable } from '@/composables/useWindowControls'
import WindowControls from '../WindowControls.vue'

/**
 * Кнопки окна (F13).
 *
 * В прототипе это три нарисованных `<div>`; здесь проверяется, что перенос
 * пикселей не утащил за собой недоступную разметку.
 */

describe('WindowControls', () => {
  it('это настоящие кнопки с подписями, а не три картинки', () => {
    const wrapper = mount(WindowControls)

    const labels = wrapper.findAll('button').map((node) => node.attributes('aria-label'))
    expect(labels).toEqual(['Свернуть окно', 'Развернуть окно', 'Закрыть окно'])
  })

  it('ни один глиф не тянется из сети', () => {
    // Иконки собраны рамками CSS: фавиконы и шрифты из сети запрещены целиком
    // (CLAUDE.md), и на рамку окна это правило распространяется тоже.
    const wrapper = mount(WindowControls)

    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.find('svg').exists()).toBe(false)
    expect(wrapper.html()).not.toContain('url(')
  })

  it('в браузере нажатие ничего не ломает и не всплывает ошибкой', async () => {
    // Окна Tauri в деве нет. Кнопки всё равно рисуются: видимо задизейбленная
    // кнопка закрытия читается как «приложение сломано».
    expect(windowControlsAvailable.value).toBe(false)

    const wrapper = mount(WindowControls)
    for (const button of wrapper.findAll('button')) {
      await button.trigger('click')
    }

    expect(wrapper.findAll('button')).toHaveLength(3)
  })

  it('кнопки исключены из зоны перетаскивания окна', () => {
    // Иначе за них нельзя было бы нажать: полоса заголовка тянет окно мышью.
    const wrapper = mount(WindowControls)

    expect(wrapper.find('[data-tauri-drag-region]').exists()).toBe(false)
  })
})
