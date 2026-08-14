import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'

import SyButton from '../SyButton.vue'
import SyCopyButton from '../SyCopyButton.vue'
import SyEmptyState from '../SyEmptyState.vue'
import SyField from '../SyField.vue'
import SyInput from '../SyInput.vue'
import SyListItem from '../SyListItem.vue'
import SyModal from '../SyModal.vue'
import SySecretField from '../SySecretField.vue'
import SySelect from '../SySelect.vue'
import SyToggle from '../SyToggle.vue'
import { iconHue, iconInitials } from '../localIcon'
import { vaultColorVar } from '../vaultColor'

describe('SyButton', () => {
  it('вешает класс варианта и размера', () => {
    const wrapper = mount(SyButton, { props: { variant: 'danger', size: 'lg' } })

    expect(wrapper.classes()).toContain('sy-button--danger')
    expect(wrapper.classes()).toContain('sy-button--lg')
  })

  it('во время загрузки заблокирована и помечена aria-busy', async () => {
    const wrapper = mount(SyButton, { props: { loading: true } })

    expect(wrapper.attributes('disabled')).toBeDefined()
    expect(wrapper.attributes('aria-busy')).toBe('true')

    await wrapper.trigger('click')
    expect(wrapper.emitted('click')).toBeUndefined()
  })

  it('иконочный вариант помечается классом и не мешает размеру', () => {
    const wrapper = mount(SyButton, { props: { icon: true, size: 'header' } })

    expect(wrapper.classes()).toContain('sy-button--icon')
    expect(wrapper.classes()).toContain('sy-button--header')
  })

  it('без пропа icon квадратным не становится', () => {
    expect(mount(SyButton).classes()).not.toContain('sy-button--icon')
  })
})

describe('SyInput', () => {
  it('прокидывает ввод наверх через v-model', async () => {
    const wrapper = mount(SyInput, { props: { modelValue: '' } })

    await wrapper.find('input').setValue('anna@fastmail.com')

    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['anna@fastmail.com'])
  })

  it('пароль по умолчанию под маской, кнопка снимает и возвращает её', async () => {
    const wrapper = mount(SyInput, { props: { modelValue: 'секрет', type: 'password' } })
    const input = wrapper.find('input')

    expect(input.attributes('type')).toBe('password')

    await wrapper.find('.sy-input__reveal').trigger('click')
    expect(wrapper.find('input').attributes('type')).toBe('text')

    await wrapper.find('.sy-input__reveal').trigger('click')
    expect(wrapper.find('input').attributes('type')).toBe('password')
  })

  it('ошибка заменяет подсказку и помечает поле для скринридера', () => {
    const wrapper = mount(SyInput, {
      props: { modelValue: '', hint: 'подсказка', error: 'Неверный мастер-пароль.' },
    })

    expect(wrapper.text()).toContain('Неверный мастер-пароль.')
    expect(wrapper.text()).not.toContain('подсказка')
    expect(wrapper.find('input').attributes('aria-invalid')).toBe('true')
  })

  it('шлёт submit по Enter — вход открывается с клавиатуры', async () => {
    const wrapper = mount(SyInput, { props: { modelValue: 'x' } })

    await wrapper.find('input').trigger('keydown.enter')

    expect(wrapper.emitted('submit')).toHaveLength(1)
  })
})

describe('SyModal', () => {
  it('ничего не рендерит, пока закрыт', () => {
    mount(SyModal, {
      props: { open: false, title: 'Отозвать устройство?' },
      attachTo: document.body,
    })

    expect(document.body.querySelector('.sy-modal')).toBeNull()
  })

  it('показывает заголовок, предупреждение и содержимое', () => {
    const wrapper = mount(SyModal, {
      props: {
        open: true,
        title: 'Экспорт в CSV',
        tone: 'warning',
        warning: 'Файл не будет зашифрован.',
      },
      slots: { default: 'тело диалога' },
      attachTo: document.body,
    })

    const dialog = document.body.querySelector('.sy-modal__dialog')
    expect(dialog?.textContent).toContain('Экспорт в CSV')
    expect(dialog?.textContent).toContain('Файл не будет зашифрован.')
    expect(dialog?.textContent).toContain('тело диалога')

    wrapper.unmount()
  })

  it('закрывается по Escape, но не когда persistent', async () => {
    const wrapper = mount(SyModal, {
      props: { open: true, title: 'Диалог' },
      attachTo: document.body,
    })

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(wrapper.emitted('close')).toHaveLength(1)

    await wrapper.setProps({ persistent: true })
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(wrapper.emitted('close')).toHaveLength(1)

    wrapper.unmount()
  })

  it('размер задаётся назначением диалога, а не пикселями', async () => {
    const wrapper = mount(SyModal, {
      props: { open: true, title: 'Диалог' },
      attachTo: document.body,
    })

    const classes = () => [...document.body.querySelector('.sy-modal__dialog')!.classList]
    expect(classes()).toContain('sy-modal__dialog--default')

    for (const size of ['confirm', 'form', 'wizard', 'wide'] as const) {
      await wrapper.setProps({ size })
      expect(classes()).toContain(`sy-modal__dialog--${size}`)
    }

    wrapper.unmount()
  })

  it('опасный диалог обведён красным целиком, а не только полоской внутри', async () => {
    // Про потерю данных должно быть понятно раньше, чем прочитан текст.
    const wrapper = mount(SyModal, {
      props: { open: true, title: 'Удалить запись?', tone: 'danger' },
      attachTo: document.body,
    })

    const dialog = () => document.body.querySelector('.sy-modal__dialog')!
    expect([...dialog().classList]).toContain('sy-modal__dialog--tone-danger')

    await wrapper.setProps({ tone: 'neutral' })
    expect([...dialog().classList]).toContain('sy-modal__dialog--tone-neutral')

    wrapper.unmount()
  })

  it('сноска стоит рядом с кнопками, а не выше по тексту', () => {
    const wrapper = mount(SyModal, {
      props: { open: true, title: 'Удалить запись?', tone: 'danger' },
      slots: {
        note: 'Подтвердите повторным нажатием «Удалить запись»',
        actions: '<button>Удалить запись</button>',
      },
      attachTo: document.body,
    })

    const actions = document.body.querySelector('.sy-modal__actions')!
    expect(actions.querySelector('.sy-modal__note')?.textContent).toContain(
      'Подтвердите повторным нажатием',
    )
    // Селектор `.sy-modal__actions button` остаётся рабочим — на него смотрят
    // тесты экранов, подтверждающих удаление.
    expect(actions.querySelector('button')?.textContent).toBe('Удалить запись')

    wrapper.unmount()
  })

  it('без сноски лишней разметки не появляется', () => {
    const wrapper = mount(SyModal, {
      props: { open: true, title: 'Диалог' },
      slots: { actions: '<button>Ок</button>' },
      attachTo: document.body,
    })

    expect(document.body.querySelector('.sy-modal__note')).toBeNull()

    wrapper.unmount()
  })

  it('оговорка внутри опасного диалога окрашена отдельно от рамки', async () => {
    // Уровней риска два: карточка красная, а оговорка про TOTP — янтарная.
    const wrapper = mount(SyModal, {
      props: {
        open: true,
        title: 'Удалить запись?',
        tone: 'danger',
        warningTone: 'warning',
        warning: 'Вместе с записью пропадёт и её код подтверждения.',
      },
      attachTo: document.body,
    })

    const strip = () => document.body.querySelector('.sy-modal__warning')!
    expect([...strip().classList]).toContain('sy-modal__warning--warning')
    expect([...strip().classList]).not.toContain('sy-modal__warning--danger')

    // Без своего тона полоска по-прежнему идёт в тон рамке.
    await wrapper.setProps({ warningTone: undefined })
    expect([...strip().classList]).toContain('sy-modal__warning--danger')

    wrapper.unmount()
  })

  it('полосный диалог кладёт шапку, тело и кнопки в отдельные полосы', () => {
    const wrapper = mount(SyModal, {
      props: { open: true, title: 'Две версии одной записи', size: 'wide', banded: true },
      slots: {
        lead: 'Запись правили офлайн на двух устройствах.',
        default: 'две колонки',
        actions: '<button>Решить позже</button>',
      },
      attachTo: document.body,
    })

    const dialog = document.body.querySelector('.sy-modal__dialog')!
    expect([...dialog.classList]).toContain('sy-modal__dialog--banded')

    const head = dialog.querySelector('.sy-modal__head')!
    expect(head.querySelector('.sy-modal__title')?.textContent).toBe('Две версии одной записи')
    expect(head.querySelector('.sy-modal__lead')?.textContent).toContain('правили офлайн')

    // Тело и кнопки — соседние полосы, а не вложенные друг в друга.
    expect(dialog.querySelector('.sy-modal__content')?.textContent).toContain('две колонки')
    expect(dialog.querySelector('.sy-modal__content .sy-modal__actions')).toBeNull()

    wrapper.unmount()
  })

  it('без слота lead подзаголовка в шапке нет', () => {
    const wrapper = mount(SyModal, {
      props: { open: true, title: 'Диалог' },
      attachTo: document.body,
    })

    expect(document.body.querySelector('.sy-modal__lead')).toBeNull()

    wrapper.unmount()
  })

  it('фокус не уходит за пределы диалога по Tab', async () => {
    const wrapper = mount(SyModal, {
      props: { open: true, title: 'Удалить запись?' },
      slots: { actions: '<button>Отмена</button><button>Удалить</button>' },
      attachTo: document.body,
    })
    await nextTick()

    const buttons = [
      ...document.body.querySelectorAll<HTMLButtonElement>('.sy-modal__actions button'),
    ]
    const [first, last] = [buttons[0]!, buttons[buttons.length - 1]!]

    // От диалога Shift+Tab уводит на последнюю кнопку, а не на подложку.
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true }))
    expect(document.activeElement).toBe(last)

    last.focus()
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }))
    expect(document.activeElement).toBe(first)

    wrapper.unmount()
  })

  it('после закрытия фокус возвращается туда, откуда диалог открыли', async () => {
    const opener = document.createElement('button')
    document.body.append(opener)
    opener.focus()

    const wrapper = mount(SyModal, {
      props: { open: false, title: 'Диалог' },
      attachTo: document.body,
    })

    await wrapper.setProps({ open: true })
    await nextTick()
    expect(document.activeElement).not.toBe(opener)

    await wrapper.setProps({ open: false })
    expect(document.activeElement).toBe(opener)

    wrapper.unmount()
    opener.remove()
  })
})

describe('SyEmptyState', () => {
  it('показывает заголовок, описание и действия', () => {
    const wrapper = mount(SyEmptyState, {
      props: { title: 'Здесь пока пусто', description: 'Добавьте первую запись.' },
      slots: { actions: '<button>Новая запись</button>' },
    })

    expect(wrapper.text()).toContain('Здесь пока пусто')
    expect(wrapper.text()).toContain('Добавьте первую запись.')
    expect(wrapper.find('button').text()).toBe('Новая запись')
  })
})

describe('SyListItem', () => {
  it('показывает метаданные записи и метку аккаунта', () => {
    const wrapper = mount(SyListItem, {
      props: { title: 'GitHub', subtitle: 'demo-user', badge: 'рабочий' },
    })

    expect(wrapper.text()).toContain('GitHub')
    expect(wrapper.text()).toContain('demo-user')
    expect(wrapper.text()).toContain('рабочий')
  })

  it('не даёт положить секрет: у строки нет пропа под пароль', () => {
    // Пароль в строке не показывается никогда (§3.1). Проверяем, что лишний
    // атрибут просто оседает на корне как обычный HTML-атрибут и в тексте
    // строки не появляется — компонент не умеет его отрисовать.
    const wrapper = mount(SyListItem, {
      props: { title: 'GitHub', subtitle: 'demo-user' },
      attrs: { 'data-x': 'mock-github-pw' },
    })

    expect(wrapper.text()).not.toContain('mock-github-pw')
  })

  it('рисует локальную плитку без единого сетевого запроса', () => {
    const wrapper = mount(SyListItem, { props: { title: 'GitHub', seed: 'github.com' } })
    const icon = wrapper.find('.sy-list-item__icon')

    expect(icon.text()).toBe('GH')
    // Ни <img>, ни background-image: фавиконы из сети не тянем никогда.
    expect(wrapper.find('img').exists()).toBe(false)
    expect(icon.attributes('style')).not.toContain('url(')
  })
})

describe('SyCopyButton', () => {
  it('показывает отсчёт очистки буфера прямо в кнопке', () => {
    const wrapper = mount(SyCopyButton, {
      props: { label: 'Копировать пароль', copied: true, seconds: 14 },
    })

    expect(wrapper.text()).toBe('Скопировано · 14 с')
    expect(wrapper.classes()).toContain('sy-copy--copied')
  })

  it('честно называет крайние состояния и не даёт нажать', () => {
    const empty = mount(SyCopyButton, { props: { label: 'Копировать', empty: true } })
    expect(empty.text()).toBe('Нечего копировать')
    expect(empty.attributes('disabled')).toBeDefined()

    const broken = mount(SyCopyButton, { props: { label: 'Копировать', unavailable: true } })
    expect(broken.text()).toBe('Буфер недоступен')
    expect(broken.attributes('disabled')).toBeDefined()
  })
})

describe('SySecretField', () => {
  it('маскирует значение фиксированной длиной: длина пароля — тоже секрет', () => {
    const short = mount(SySecretField, { props: { label: 'Пароль' } })
    const long = mount(SySecretField, { props: { label: 'Пароль' } })

    expect(short.find('.sy-secret__mask').text()).toBe(long.find('.sy-secret__mask').text())
    expect(short.text()).toContain('копировать можно, не открывая')
  })

  it('открытое значение сопровождает обещанием закрыться', () => {
    const wrapper = mount(SySecretField, {
      props: { label: 'Пароль', value: 'секрет-на-экране', hideIn: 25 },
    })

    expect(wrapper.find('.sy-secret__value').text()).toBe('секрет-на-экране')
    expect(wrapper.text()).toContain('скроется автоматически через 25 с')
    expect(wrapper.find('.sy-secret__toggle').text()).toBe('Скрыть')
  })

  it('у пустого поля нечего открывать', () => {
    const wrapper = mount(SySecretField, {
      props: { label: 'Заметки', present: false, emptyText: 'Заметок нет' },
    })

    expect(wrapper.text()).toBe('ЗаметкиЗаметок нет')
    expect(wrapper.find('.sy-secret__toggle').exists()).toBe(false)
    expect(wrapper.find('.sy-copy').exists()).toBe(false)
  })

  it('закрытое поле говорит, ЧТО скопирует; открытое — просто «Копировать»', async () => {
    const wrapper = mount(SySecretField, {
      props: { label: 'Пароль', copyLabel: 'Копировать пароль' },
    })

    expect(wrapper.find('.sy-copy').text()).toBe('Копировать пароль')

    await wrapper.setProps({ value: 'секрет-на-экране' })
    expect(wrapper.find('.sy-copy').text()).toBe('Копировать')
  })

  it('скелет заменяет маску и оставляет одно действие на всё поле', async () => {
    const wrapper = mount(SySecretField, { props: { label: 'Заметки', skeleton: true } })

    // Ни маски, ни копирования, ни отдельной кнопки внутри бокса.
    expect(wrapper.find('.sy-secret__mask').exists()).toBe(false)
    expect(wrapper.find('.sy-copy').exists()).toBe(false)
    expect(wrapper.find('.sy-secret__toggle').exists()).toBe(false)
    expect(wrapper.findAll('.sy-secret__bar')).toHaveLength(2)

    await wrapper.find('.sy-secret__box--button').trigger('click')
    expect(wrapper.emitted('toggle')).toHaveLength(1)

    await wrapper.setProps({ value: 'заметка на экране' })
    expect(wrapper.find('.sy-secret__value').text()).toBe('заметка на экране')
    expect(wrapper.find('.sy-secret__bar').exists()).toBe(false)
  })
})

describe('SyToggle', () => {
  it('это switch: состояние читается и голосом, и словами', () => {
    const wrapper = mount(SyToggle, {
      props: {
        modelValue: false,
        label: 'Синхронизировать',
        stateText: 'только здесь',
      },
    })

    const control = wrapper.find('.sy-toggle__switch')
    expect(control.attributes('role')).toBe('switch')
    expect(control.attributes('aria-checked')).toBe('false')
    expect(wrapper.text()).toContain('только здесь')
  })

  it('шлёт наверх противоположное значение, а сам не переключается', async () => {
    const wrapper = mount(SyToggle, { props: { modelValue: true, label: 'Синхронизировать' } })

    await wrapper.find('.sy-toggle__switch').trigger('click')

    // Состояние приходит из ответа ядра, а не «оптимистично» из клика.
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual([false])
    expect(wrapper.find('.sy-toggle__switch').attributes('aria-checked')).toBe('true')
  })

  it('пока ядро отвечает, нажать нельзя', async () => {
    const wrapper = mount(SyToggle, {
      props: { modelValue: false, label: 'Синхронизировать', busy: true },
    })

    expect(wrapper.find('.sy-toggle__switch').attributes('disabled')).toBeDefined()
    await wrapper.find('.sy-toggle__switch').trigger('click')
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
  })
})

describe('SyField', () => {
  it('подпись связана с контролом внутри, хотя `for` нигде не проставлен', () => {
    // Ради этого подпись — настоящий `<label>` с `display: contents`: сетке он
    // не мешает, а диктору поле перестаёт быть «текстовым полем» без имени.
    const wrapper = mount(SyField, {
      props: { label: 'Пароль' },
      slots: { default: '<input type="password" />' },
      attachTo: document.body,
    })

    const input = wrapper.find<HTMLInputElement>('input').element
    expect(input.labels?.[0]?.textContent?.trim()).toBe('Пароль')

    wrapper.unmount()
  })

  it('место под действие занимает, только когда действие дали', () => {
    const plain = mount(SyField, { props: { label: 'Пароль' } })
    expect(plain.find('.sy-field__action').exists()).toBe(false)

    const withAction = mount(SyField, {
      props: { label: 'Пароль' },
      slots: { action: '<button type="button">Показать текущий</button>' },
    })
    expect(withAction.find('.sy-field__action').text()).toBe('Показать текущий')
  })

  it('ошибку в поле видно по подписи', () => {
    const wrapper = mount(SyField, { props: { label: 'Пароль', invalid: true } })

    expect(wrapper.find('.sy-field__label').classes()).toContain('sy-field__label--invalid')
  })
})

describe('SySelect', () => {
  const options = [
    { value: 'personal', label: 'Личное' },
    { value: 'work', label: 'Рабочее · локальная' },
  ]

  it('показывает выбранное и прокидывает выбор наверх', async () => {
    const wrapper = mount(SySelect, {
      props: { modelValue: 'personal', label: 'Секция', options },
    })

    expect(wrapper.find('.sy-select__value-text').text()).toBe('Личное')

    await wrapper.find('.sy-select__trigger').trigger('click')
    await wrapper.findAll('.sy-select__option')[1]!.trigger('click')

    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['work'])
  })

  it('без вариантов нечего выбирать', () => {
    const wrapper = mount(SySelect, { props: { modelValue: '', options: [] } })

    expect(wrapper.find('.sy-select__trigger').attributes('disabled')).toBeDefined()
  })

  it('список раскрывается и закрывается, и сам по себе ничего не выбирает', async () => {
    const wrapper = mount(SySelect, { props: { modelValue: 'personal', options } })
    const trigger = wrapper.find('.sy-select__trigger')

    expect(trigger.attributes('aria-expanded')).toBe('false')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)

    await trigger.trigger('click')

    expect(trigger.attributes('aria-expanded')).toBe('true')
    expect(wrapper.findAll('[role="option"]')).toHaveLength(2)
    expect(wrapper.find('.sy-select__option--on').text()).toBe('Личное')

    await trigger.trigger('click')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
  })

  it('стрелки водят подсветку, а выбирает только Enter', async () => {
    // Иначе проход стрелками молча менял бы секцию записи по дороге.
    const wrapper = mount(SySelect, { props: { modelValue: 'personal', options } })
    const trigger = wrapper.find('.sy-select__trigger')

    await trigger.trigger('keydown', { key: 'ArrowDown' })
    expect(wrapper.find('[role="listbox"]').exists()).toBe(true)

    await trigger.trigger('keydown', { key: 'ArrowDown' })
    expect(wrapper.findAll('.sy-select__option')[1]!.classes()).toContain(
      'sy-select__option--active',
    )
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()

    await trigger.trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['work'])
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
  })

  it('Escape закрывает список и не идёт дальше по форме', async () => {
    // Форма записи сама закрывается по Escape: без stopPropagation одно нажатие
    // закрыло бы и список, и всю форму.
    const wrapper = mount(SySelect, {
      props: { modelValue: 'personal', options },
      attachTo: document.body,
    })
    const outer = vi.fn()
    document.body.addEventListener('keydown', outer)

    await wrapper.find('.sy-select__trigger').trigger('click')
    wrapper
      .find('.sy-select__trigger')
      .element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await wrapper.vm.$nextTick()

    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
    expect(outer).not.toHaveBeenCalled()

    document.body.removeEventListener('keydown', outer)
    wrapper.unmount()
  })

  it('щелчок мимо закрывает список', async () => {
    const wrapper = mount(SySelect, {
      props: { modelValue: 'personal', options },
      attachTo: document.body,
    })

    await wrapper.find('.sy-select__trigger').trigger('click')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(true)

    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    await wrapper.vm.$nextTick()

    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('цветная метка варианта доезжает и до триггера, и до списка', async () => {
    const wrapper = mount(SySelect, {
      props: {
        modelValue: 'personal',
        options: [{ value: 'personal', label: 'Личное', dot: 'var(--sy-vault-mint)' }],
      },
    })

    expect(wrapper.find('.sy-select__trigger .sy-select__dot').attributes('style')).toContain(
      'var(--sy-vault-mint)',
    )

    await wrapper.find('.sy-select__trigger').trigger('click')
    expect(wrapper.find('.sy-select__option .sy-select__dot').exists()).toBe(true)
  })

  it('подвал показывается только когда его дали', async () => {
    const withFooter = mount(SySelect, {
      props: { modelValue: 'personal', options },
      slots: { footer: '<button type="button">Управление секциями…</button>' },
    })
    await withFooter.find('.sy-select__trigger').trigger('click')
    expect(withFooter.find('.sy-select__footer').text()).toBe('Управление секциями…')

    const plain = mount(SySelect, { props: { modelValue: 'personal', options } })
    await plain.find('.sy-select__trigger').trigger('click')
    expect(plain.find('.sy-select__footer').exists()).toBe(false)
  })
})

describe('цвета секций', () => {
  it('отдаёт токен темы, а не готовый цвет', () => {
    // Цвет метки зависит от темы: в светлой те же метки темнее. Хардкод здесь
    // сломал бы контраст в одной из тем.
    expect(vaultColorVar('indigo')).toBe('var(--sy-vault-indigo)')
    expect(vaultColorVar('coral')).toBe('var(--sy-vault-coral)')
  })
})

describe('локальные иконки', () => {
  it('дают стабильный тон для одной и той же строки', () => {
    expect(iconHue('github.com')).toBe(iconHue('GitHub.com '))
    expect(iconHue('github.com')).not.toBe(iconHue('github.com'.repeat(2)))
  })

  it('держат тон в восьми фиксированных ступенях', () => {
    const allowed = [32, 78, 128, 172, 218, 262, 292, 320]
    for (const seed of ['google.com', 'notion.so', 'яндекс', 'vpn.local', '']) {
      expect(allowed).toContain(iconHue(seed))
    }
  })

  it('берёт 1–2 буквы из имени', () => {
    expect(iconInitials('GitHub')).toBe('GH')
    expect(iconInitials('Google')).toBe('G')
    expect(iconInitials('vpn.local')).toBe('VL')
    expect(iconInitials('  ')).toBe('?')
  })
})
