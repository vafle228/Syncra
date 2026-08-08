import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import type { RecordMeta } from '@/core/contract'
import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_VAULT_WORK, type MockCoreClient } from '@/core/mock'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useRecordsStore } from '@/stores/useRecordsStore'
import RecordCard from '../RecordCard.vue'

/**
 * Карточка записи (F5). Здесь проверяется главное обещание экрана: секрет
 * появляется только по нажатию, копируется без показа и не оседает нигде.
 */

let core: MockCoreClient
const writes: string[] = []

const GITHUB = '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1003'
const STEAM = '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1004'
/** Запись в локальной секции «Рабочее» — и единственная с двумя адресами. */
const GOOGLE_WORK = '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1002'

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)

  writes.length = 0
  vi.stubGlobal('navigator', {
    clipboard: {
      writeText: vi.fn(async (text: string) => {
        writes.push(text)
      }),
    },
  })
})

afterEach(() => {
  setCoreClient(null)
  vi.unstubAllGlobals()
})

async function mountCard(recordId = GITHUB) {
  const list = useRecordsStore()
  await list.load()
  const record = list.records.find((item) => item.record_id === recordId) as RecordMeta

  const wrapper = mount(RecordCard, { props: { record }, attachTo: document.body })
  await flushPromises()
  return { wrapper, list, record }
}

/** Найти поле-секрет по подписи («Пароль», «Заметки», «Ключ TOTP»). */
function secretField(wrapper: ReturnType<typeof mount>, label: string) {
  const field = wrapper
    .findAll('.sy-secret')
    .find((node) => node.find('.sy-secret__label').text() === label)
  if (!field) throw new Error(`Поле «${label}» не найдено`)
  return field
}

describe('RecordCard · метаданные', () => {
  it('показывает метаданные записи и ни одного секрета', async () => {
    const { wrapper } = await mountCard()

    expect(wrapper.text()).toContain('GitHub')
    expect(wrapper.text()).toContain('demo-user')
    expect(wrapper.text()).toContain('github.com')

    expect(wrapper.html()).not.toContain('mock-github-pw')
    expect(wrapper.html()).not.toContain('MOCKTOTPSECRET')
    expect(wrapper.html()).not.toContain('Recovery codes')

    wrapper.unmount()
  })

  it('различает «поле пустое» и «поле скрыто», не читая секретов', async () => {
    // У Steam нет ни заметок, ни ключа TOTP — знаем это из метаданных.
    const { wrapper } = await mountCard(STEAM)

    expect(secretField(wrapper, 'Заметки').text()).toContain('Заметок нет')
    expect(secretField(wrapper, 'Ключ TOTP').text()).toContain('Не подключён')
    // Пустое поле нечего открывать — кнопки «Показать» у него нет.
    expect(secretField(wrapper, 'Заметки').find('.sy-secret__toggle').exists()).toBe(false)

    wrapper.unmount()
  })

  it('показывает секцию записи и говорит, уезжает ли она (F7, §4.2)', async () => {
    const { wrapper } = await mountCard()

    expect(wrapper.text()).toContain('Личное')
    expect(wrapper.text()).toContain('синхронизируется')

    wrapper.unmount()
  })

  it('прямо говорит, что у записи локальной секции нет копии на других устройствах', async () => {
    const list = useRecordsStore()
    await list.load()
    const record = list.records.find((item) => item.vault_id === MOCK_VAULT_WORK) as RecordMeta

    const wrapper = mount(RecordCard, { props: { record }, attachTo: document.body })
    await flushPromises()

    expect(wrapper.find('.card__foot-note').text()).toContain('копии этой записи на других')
    expect(wrapper.text()).toContain('только это устройство')

    wrapper.unmount()
  })

  it('копирует логин, не трогая буфер таймером: там нет секрета', async () => {
    const { wrapper } = await mountCard()

    const buttons = wrapper.findAll('.card__value .sy-copy')
    await buttons[buttons.length - 1]!.trigger('click')
    await flushPromises()

    expect(writes).toEqual(['demo-user'])

    wrapper.unmount()
  })
})

describe('RecordCard · reveal (ЗАКОН №1)', () => {
  it('пароль появляется только после нажатия «Показать»', async () => {
    const { wrapper } = await mountCard()
    const password = secretField(wrapper, 'Пароль')

    expect(password.text()).toContain('••••••••••')
    expect(wrapper.html()).not.toContain('mock-github-pw')

    await password.find('.sy-secret__toggle').trigger('click')
    await flushPromises()

    expect(wrapper.html()).toContain('mock-github-pw')
    expect(password.text()).toContain('скроется автоматически через 30 с')

    wrapper.unmount()
  })

  it('открытый пароль не попадает в состояние приложения', async () => {
    const { wrapper, list } = await mountCard()

    await secretField(wrapper, 'Пароль').find('.sy-secret__toggle').trigger('click')
    await flushPromises()

    expect(wrapper.html()).toContain('mock-github-pw')
    expect(JSON.stringify(list.$state)).not.toContain('mock-github-pw')

    wrapper.unmount()
  })

  it('копирует пароль, не показывая его, и обещает очистить буфер', async () => {
    const { wrapper } = await mountCard()
    const password = secretField(wrapper, 'Пароль')

    await password.find('.sy-copy').trigger('click')
    await flushPromises()

    expect(writes).toEqual(['mock-github-pw'])
    // Значение ушло в буфер, но на экране его так и не было.
    expect(wrapper.html()).not.toContain('mock-github-pw')
    expect(password.find('.sy-copy').text()).toContain('Скопировано · 20')

    wrapper.unmount()
  })

  it('прячет открытое, когда хранилище заблокировалось', async () => {
    const { wrapper } = await mountCard()
    await secretField(wrapper, 'Пароль').find('.sy-secret__toggle').trigger('click')
    await flushPromises()
    expect(wrapper.html()).toContain('mock-github-pw')

    core.control.forceLock('timeout')
    await flushPromises()

    expect(wrapper.html()).not.toContain('mock-github-pw')

    wrapper.unmount()
  })
})

describe('RecordCard · удаление', () => {
  it('спрашивает подтверждение и удаляет запись в ядре', async () => {
    const { wrapper, list } = await mountCard()

    await wrapper.find('.card__foot .sy-button--danger').trigger('click')
    await flushPromises()

    const dialog = document.body.querySelector('.sy-modal__dialog')
    expect(dialog?.textContent).toContain('Удалить «GitHub»?')
    expect(dialog?.textContent).toContain('Восстановить её из Syncra будет нельзя')
    // Запись с TOTP — предупреждение про код подтверждения обязано быть.
    expect(dialog?.textContent).toContain('пропадёт и её код подтверждения')
    expect(list.total).toBe(4)

    const confirm = [...document.body.querySelectorAll('.sy-modal__actions button')].find((node) =>
      node.textContent?.includes('Удалить запись'),
    ) as HTMLButtonElement
    confirm.click()
    await flushPromises()

    expect(list.total).toBe(3)
    expect(list.records.some((record) => record.record_id === GITHUB)).toBe(false)

    wrapper.unmount()
  })

  it('не пугает предупреждением про TOTP там, где его нет', async () => {
    const { wrapper } = await mountCard(STEAM)

    await wrapper.find('.card__foot .sy-button--danger').trigger('click')
    await flushPromises()

    const dialog = document.body.querySelector('.sy-modal__dialog')
    expect(dialog?.textContent).toContain('Удалить «Steam»?')
    expect(dialog?.textContent).not.toContain('код подтверждения')

    wrapper.unmount()
  })

  it('оставляет запись на месте, если ядро отказало', async () => {
    const { wrapper, list } = await mountCard()
    core.control.failNext('INTERNAL', 'Хранилище занято.')

    await wrapper.find('.card__foot .sy-button--danger').trigger('click')
    await flushPromises()
    const confirm = [...document.body.querySelectorAll('.sy-modal__actions button')].find((node) =>
      node.textContent?.includes('Удалить запись'),
    ) as HTMLButtonElement
    confirm.click()
    await flushPromises()

    expect(list.total).toBe(4)
    expect(document.body.querySelector('.sy-modal__dialog')?.textContent).toContain(
      'Хранилище занято.',
    )

    wrapper.unmount()
  })
})

describe('конфликт версий (F11)', () => {
  it('предупреждает о споре над карточкой и открывает обе версии', async () => {
    const { wrapper } = await mountCard()

    const banner = wrapper.find('.conflict-banner')
    expect(banner.exists()).toBe(true)
    expect(banner.text()).toContain('Эту запись правили на двух устройствах')
    expect(banner.text()).toContain('iPhone 14')

    await banner.find('.conflict-banner__action').trigger('click')
    await flushPromises()

    expect(document.body.querySelector('.sy-modal__dialog')?.textContent).toContain(
      'Две версии одной записи',
    )

    wrapper.unmount()
  })

  it('у записи без спора полосы нет', async () => {
    const { wrapper } = await mountCard(STEAM)

    expect(wrapper.find('.conflict-banner').exists()).toBe(false)

    wrapper.unmount()
  })

  it('после выбора версии полоса уходит вместе с конфликтом', async () => {
    const { wrapper } = await mountCard()
    await useConflictsStore().resolve(GITHUB, 'local')
    await flushPromises()

    expect(wrapper.find('.conflict-banner').exists()).toBe(false)

    wrapper.unmount()
  })
})

describe('подвал карточки (F10)', () => {
  it('говорит, что изменение ещё не уехало', async () => {
    const { wrapper, record } = await mountCard(STEAM)
    await useRecordsStore().update(record.record_id, { login: 'demo_player_2' })
    await flushPromises()

    expect(wrapper.find('.card__foot-note').text()).toContain('уедет, когда рядом')

    wrapper.unmount()
  })
})

describe('RecordCard · адреса (F13)', () => {
  /**
   * Смонтировать карточку с заданным набором адресов.
   *
   * Проп — снимок записи, а не ссылка в стор: `update()` кладёт туда НОВЫЙ
   * объект, поэтому свежую версию надо передать карточке явно, как это делает
   * `VaultView` через `list.selected`.
   */
  async function mountWithUrls(urls: string[], recordId = GOOGLE_WORK) {
    const mounted = await mountCard(recordId)
    const updated = await useRecordsStore().update(recordId, { urls })
    await mounted.wrapper.setProps({ record: updated })
    await flushPromises()
    return mounted
  }

  it('показывает имя сайта, а копирует полный адрес', async () => {
    // Сид хранит адреса без схемы; после правки в форме там появляется путь —
    // именно его надо отдать тому, кто адрес забирает.
    const { wrapper } = await mountWithUrls([
      'https://admin.google.com/u/0/ac/users',
      'accounts.google.com',
    ])

    const primary = wrapper.find('.card__value-text')
    expect(primary.text()).toBe('admin.google.com')
    // Полный адрес не потерян — он в подсказке.
    expect(primary.attributes('title')).toBe('https://admin.google.com/u/0/ac/users')

    await wrapper.find('.card__value .sy-copy').trigger('click')
    await flushPromises()

    expect(writes).toEqual(['https://admin.google.com/u/0/ac/users'])

    wrapper.unmount()
  })

  it('прячет лишние адреса за «ещё N», а не растит карточку', async () => {
    const { wrapper } = await mountWithUrls([
      'a.example',
      'b.example',
      'c.example',
      'd.example',
      'e.example',
    ])

    // Первый адрес — строкой, два следующих — чипами, остальные скрыты.
    expect(wrapper.findAll('.card__chip')).toHaveLength(2)
    expect(wrapper.find('.card__more').text()).toBe('ещё 2')

    await wrapper.find('.card__more').trigger('click')
    expect(wrapper.findAll('.card__chip')).toHaveLength(4)
    expect(wrapper.find('.card__more').text()).toBe('свернуть')

    wrapper.unmount()
  })

  it('чип копирует свой адрес целиком', async () => {
    const { wrapper } = await mountCard(GOOGLE_WORK)

    await wrapper.find('.card__chip').trigger('click')
    await flushPromises()

    expect(writes).toEqual(['admin.google.com'])

    wrapper.unmount()
  })

  it('говорит, когда адреса нет вообще', async () => {
    const { wrapper } = await mountWithUrls([], GITHUB)

    expect(wrapper.find('.card__value--empty').text()).toContain('автозаполнению не за что')
    expect(wrapper.find('.card__chip').exists()).toBe(false)

    wrapper.unmount()
  })
})

describe('RecordCard · плашка состояния в шапке (F13)', () => {
  it('конфликт заслоняет всё остальное', async () => {
    const { wrapper } = await mountCard(GITHUB)

    expect(wrapper.find('.card__sync').text()).toBe('конфликт версий')
    expect(wrapper.find('.card__sync').classes()).toContain('card__sync--danger')

    wrapper.unmount()
  })

  it('про локальную секцию говорит прямо: копии нигде нет', async () => {
    const { wrapper } = await mountCard(GOOGLE_WORK)

    // «Только это устройство» важнее «ждёт синхронизации»: запись не уедет
    // никогда, а не «пока».
    expect(wrapper.find('.card__sync').text()).toBe('только это устройство')

    wrapper.unmount()
  })

  it('спокойная запись подтверждает, что копии есть', async () => {
    const { wrapper } = await mountCard(STEAM)

    expect(wrapper.find('.card__sync').text()).toBe('синхронизировано')

    wrapper.unmount()
  })
})

describe('RecordCard · меню «···» (F13)', () => {
  it('открывается по нажатию и честно говорит, что состав не решён', async () => {
    const { wrapper } = await mountCard()
    const button = wrapper.find('.card__menu-button')

    expect(button.attributes('aria-expanded')).toBe('false')
    expect(wrapper.find('.card__menu-pop').exists()).toBe(false)

    await button.trigger('click')

    expect(button.attributes('aria-expanded')).toBe('true')
    expect(wrapper.find('.card__menu-pop').text()).toContain('Ещё думаем')
    // Обещание реплики: рабочие действия не прячутся в меню.
    expect(wrapper.find('.card__menu-pop').text()).toContain('лежит на виду')

    await button.trigger('click')
    expect(wrapper.find('.card__menu-pop').exists()).toBe(false)

    wrapper.unmount()
  })

  it('закрывается по Escape и щелчком мимо, а не только тем же «···»', async () => {
    // Иначе поповер — ловушка: кнопка, которой его открыли, за ним же и скрыта.
    const { wrapper } = await mountCard()

    await wrapper.find('.card__menu-button').trigger('click')
    expect(wrapper.find('.card__menu-pop').exists()).toBe(true)

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(wrapper.find('.card__menu-pop').exists()).toBe(false)

    await wrapper.find('.card__menu-button').trigger('click')
    expect(wrapper.find('.card__menu-pop').exists()).toBe(true)

    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    await flushPromises()
    expect(wrapper.find('.card__menu-pop').exists()).toBe(false)

    wrapper.unmount()
  })

  it('удаление осталось в подвале, а не уехало в меню', async () => {
    // Существующий тест ищет кнопку удаления там же — и должен продолжать.
    const { wrapper } = await mountCard()

    expect(wrapper.find('.card__foot .sy-button--danger').text()).toBe('Удалить запись')

    wrapper.unmount()
  })
})
