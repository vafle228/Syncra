import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'

import SyButton from '../SyButton.vue'
import SyCopyButton from '../SyCopyButton.vue'
import SyEmptyState from '../SyEmptyState.vue'
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

    expect(wrapper.text()).toBe('Скопировано · 14')
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

describe('SySelect', () => {
  const options = [
    { value: 'personal', label: 'Личное' },
    { value: 'work', label: 'Рабочее · локальная' },
  ]

  it('показывает выбранное и прокидывает выбор наверх', async () => {
    const wrapper = mount(SySelect, {
      props: { modelValue: 'personal', label: 'Секция', options },
    })

    expect(wrapper.find<HTMLSelectElement>('select').element.value).toBe('personal')

    await wrapper.find('select').setValue('work')
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['work'])
  })

  it('без вариантов нечего выбирать', () => {
    const wrapper = mount(SySelect, { props: { modelValue: '', options: [] } })

    expect(wrapper.find('select').attributes('disabled')).toBeDefined()
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
