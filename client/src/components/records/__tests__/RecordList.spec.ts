import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useSyncStore } from '@/stores/useSyncStore'
import { useVaultUiStore } from '@/stores/useVaultUiStore'
import { mountWithRouter } from '@/test/mountWithRouter'
import RecordList from '../RecordList.vue'

/**
 * Средняя панель окна (F4, F13) — то, что до F13 было половиной `HomeView`.
 *
 * Панель монтируется НАПРЯМУЮ: `RouterView` внутри неё нет, а данные сеанса в
 * приложении поднимает оболочка — здесь их поднимает тест. Так спек проверяет
 * список, а не порядок загрузки.
 */

afterEach(() => {
  setCoreClient(null)
  vi.unstubAllGlobals()
})

/** Смонтировать панель на открытом хранилище и поднять данные, как это делает оболочка. */
async function mountList(core?: MockCoreClient) {
  const harness = await mountWithRouter(RecordList, { core })

  await useRecordsStore().load()
  await useSectionsStore().ensure()
  await useConflictsStore().ensure()
  await useSyncStore().ensure()
  await flushPromises()

  return harness
}

async function search(wrapper: VueWrapper, value: string) {
  const input = wrapper.find<HTMLInputElement>('.record-list__search-input')
  input.element.value = value
  await input.trigger('input')
}

describe('RecordList · список (сквозной путь UI → IPC → мок-ядро)', () => {
  it('рендерит метаданные записей из мок-ядра', async () => {
    const { wrapper } = await mountList()

    const text = wrapper.text()
    expect(text).toContain('Google')
    expect(text).toContain('personal.demo@gmail.com')
    expect(text).toContain('Рабочий')
  })

  it('не выводит на экран ни одного секрета', async () => {
    const { wrapper } = await mountList()

    // Все сид-секреты помечены префиксом `mock-` / MOCKTOTP — ничего из этого
    // не должно попасть в разметку списка (Закон №1).
    expect(wrapper.html()).not.toMatch(/mock-[a-z]+-pw/)
    expect(wrapper.html()).not.toContain('MOCKTOTPSECRET')
    expect(wrapper.html()).not.toContain('Recovery codes')
  })

  it('не показывает удалённую запись (tombstone)', async () => {
    const { wrapper } = await mountList()

    expect(wrapper.text()).not.toContain('Jira')
    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(4)
  })

  it('показывает счётчик записей и сервисов и обещает, что пароли скрыты', async () => {
    const { wrapper } = await mountList()

    expect(wrapper.find('.record-list__count').text()).toContain('4 записи · 3 сервиса')
    // Обещание прототипа стоит рядом со счётчиком, а не в подсказке: это
    // главное, чем список Syncra отличается от таблицы паролей.
    expect(wrapper.find('.record-list__count').text()).toContain('пароли скрыты')
  })

  it('показывает скелет, пока ядро не ответило', async () => {
    // Здесь важен именно кадр ДО ответа ядра: панель смонтирована, а данные
    // ещё никто не поднимал.
    const { wrapper } = await mountWithRouter(RecordList)

    expect(wrapper.find('.record-list__skeleton').exists()).toBe(true)
    expect(wrapper.find('.record-list__list').exists()).toBe(false)

    await useRecordsStore().load()
    await flushPromises()

    expect(wrapper.find('.record-list__skeleton').exists()).toBe(false)
    expect(wrapper.find('.record-list__list').exists()).toBe(true)
  })

  it('показывает сообщение ядра вместо списка, если ядро вернуло ошибку', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    const { wrapper } = await mountList(core)

    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    await useRecordsStore().load()
    await flushPromises()

    expect(wrapper.text()).toContain('Ядро недоступно.')
    expect(wrapper.find('.record-list__list').exists()).toBe(false)
    expect(wrapper.find('.record-list__skeleton').exists()).toBe(false)
  })

  it('показывает пустое состояние на свежесозданном хранилище', async () => {
    const core = createMockCoreClient({ latencyMs: 0, initialized: false })
    // Хранилище создаётся ДО монтирования: иначе хранитель уведёт в онбординг.
    await core.initVault('рыжий трамвай у моста')

    const { wrapper } = await mountList(core)

    expect(wrapper.text()).toContain('Пока ни одного пароля')
    expect(wrapper.find('.record-list__list').exists()).toBe(false)
  })
})

describe('RecordList · синхронизация и конфликты (F10, F11)', () => {
  it('помечает запись, из-за которой спор, и ту, что ещё не уехала', async () => {
    const { wrapper } = await mountList()

    const conflictRow = wrapper
      .findAll('[data-test="row"]')
      .find((row) => row.text().includes('GitHub'))
    expect(conflictRow?.find('.sy-list-item__status--conflict').text()).toContain('конфликт версий')

    // Правка другой записи ждёт отправки — и это видно на её строке.
    const steam = useRecordsStore().records.find((record) => record.service_name === 'Steam')!
    await useRecordsStore().update(steam.record_id, { login: 'demo_player_2' })
    await flushPromises()

    const pendingRow = wrapper
      .findAll('[data-test="row"]')
      .find((row) => row.text().includes('Steam'))
    expect(pendingRow?.find('.sy-list-item__status--pending').text()).toContain(
      'ждёт синхронизации',
    )
  })
})

describe('RecordList · группировка нескольких аккаунтов (§4.4)', () => {
  it('показывает заголовок группы там, где у сервиса несколько аккаунтов', async () => {
    const { wrapper } = await mountList()

    const multi = wrapper.findAll('.record-list__group--multi')
    expect(multi).toHaveLength(1)

    const head = multi[0]?.find('.record-list__group-head')
    expect(head?.text()).toContain('Google')
    expect(head?.text()).toContain('2 аккаунта')
    expect(multi[0]?.findAll('li')).toHaveLength(2)
  })

  it('одиночная запись остаётся обычной строкой, без заголовка группы', async () => {
    const { wrapper } = await mountList()

    const heads = wrapper.findAll('.record-list__group-head')
    expect(heads).toHaveLength(1)
    expect(heads[0]?.text()).toContain('Google')

    // GitHub и Steam — по одному аккаунту, заголовков у них нет.
    expect(wrapper.text()).toContain('GitHub')
    expect(wrapper.text()).toContain('Steam')
  })
})

describe('RecordList · поиск', () => {
  it('сужает список по имени сервиса', async () => {
    const { wrapper } = await mountList()

    await search(wrapper, 'github')

    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(1)
    expect(wrapper.text()).toContain('demo-user')
    expect(wrapper.text()).not.toContain('Steam')
    expect(wrapper.find('.record-list__count').text()).toContain('найдено 1 из 4')
  })

  it('ищет по логину и по адресу сайта', async () => {
    const { wrapper } = await mountList()

    await search(wrapper, 'demo_player')
    expect(wrapper.text()).toContain('Steam')

    await search(wrapper, 'admin.google.com')
    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(1)
    expect(wrapper.text()).toContain('work.demo@syncra.example')
  })

  it('различает аккаунты одного сервиса по метке', async () => {
    const { wrapper } = await mountList()

    await search(wrapper, 'google личный')

    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(1)
    expect(wrapper.text()).toContain('personal.demo@gmail.com')
    expect(wrapper.text()).not.toContain('work.demo@syncra.example')
    // Совпал один аккаунт — заголовок группы не нужен.
    expect(wrapper.find('.record-list__group-head').exists()).toBe(false)
  })

  it('объясняет пустую выдачу и позволяет сбросить поиск', async () => {
    const { wrapper } = await mountList()

    await search(wrapper, 'dropbox')

    expect(wrapper.text()).toContain('Ничего не нашлось')
    expect(wrapper.text()).toContain('«dropbox»')
    // Обещание продукта: поиск не заглядывает в секреты и не ходит в сеть.
    expect(wrapper.text()).toContain('не заглядывает в секреты')
    expect(wrapper.find('.record-list__list').exists()).toBe(false)

    await wrapper.find('.sy-empty__actions button').trigger('click')
    await flushPromises()

    expect(wrapper.find<HTMLInputElement>('.record-list__search-input').element.value).toBe('')
    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(4)
  })

  it('поиск не находит секретов — их нечем индексировать', async () => {
    const { wrapper } = await mountList()

    await search(wrapper, 'mock-github-pw')

    expect(wrapper.text()).toContain('Ничего не нашлось')
  })
})

describe('RecordList · порядок списка', () => {
  /** Открыть выпадающий список порядка и вернуть его варианты. */
  async function openOrder(wrapper: VueWrapper) {
    await wrapper.find('[data-test="list-order"] .sy-select__trigger').trigger('click')
    return wrapper.findAll('[data-test="list-order"] .sy-select__option')
  }

  it('по умолчанию показывает недавние и не рисует подсказку про Ctrl', async () => {
    const { wrapper } = await mountList()

    expect(wrapper.find('[data-test="list-order"]').text()).toContain('Недавние')
    // Строки «Ctrl» нет ни в одном макете — сам хоткей при этом остался.
    expect(wrapper.find('.record-list__count').text()).not.toContain('Ctrl')

    // Свежее всего правили GitHub (`seed.ts`), он и стоит первым.
    expect(wrapper.findAll('[data-test="row"]')[0]?.text()).toContain('GitHub')
  })

  it('«Старые пароли» действительно переставляют список', async () => {
    const { wrapper } = await mountList()

    const options = await openOrder(wrapper)
    expect(options.map((option) => option.text())).toEqual([
      'Недавние',
      'По алфавиту',
      'Старые пароли',
    ])

    await options.find((option) => option.text() === 'Старые пароли')!.trigger('click')
    await flushPromises()

    expect(useRecordsStore().order).toBe('stale')
    // Дольше всех не менялся пароль рабочего Google — его группа встала наверх
    // (внутри группы порядок прежний, по метке: «Личный» перед «Рабочим»).
    expect(wrapper.findAll('[data-test="row"]')[0]?.text()).toContain('Google')
  })

  it('хоткей поиска работает без видимой подсказки', async () => {
    const { wrapper } = await mountList()

    const input = wrapper.find<HTMLInputElement>('.record-list__search-input').element
    const focus = vi.spyOn(input, 'focus')

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true }))

    expect(focus).toHaveBeenCalled()
  })
})

describe('RecordList · секции', () => {
  it('фильтрует список по выбранной секции и предупреждает о локальной', async () => {
    const { wrapper } = await mountList()
    const work = useSectionsStore().vaults.find((vault) => !vault.sync)!

    useRecordsStore().setVaultFilter(work.vault_id)
    await flushPromises()

    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(1)
    expect(wrapper.text()).toContain('work.demo@syncra.example')
    expect(wrapper.find('.record-list__count').text()).toContain('1 запись')
    // Про «эти записи никуда не уезжают» говорим там, где их открывают.
    expect(wrapper.find('.record-list__local').text()).toContain('не уезжают с этого устройства')

    useRecordsStore().setVaultFilter(null)
    await flushPromises()
    expect(wrapper.find('.record-list__local').exists()).toBe(false)
  })

  it('объясняет пустую секцию и даёт вернуться ко всем записям', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    const { wrapper } = await mountList(core)

    const empty = await core.createVault('Учёба', 'mint')
    await useSectionsStore().load()
    useRecordsStore().setVaultFilter(empty.vault_id)
    await flushPromises()

    expect(wrapper.text()).toContain('В этой секции пока пусто')
    expect(wrapper.find('.record-list__list').exists()).toBe(false)

    const back = wrapper
      .findAll('.sy-empty__actions button')
      .find((node) => node.text() === 'Показать все')
    await back!.trigger('click')
    await flushPromises()

    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(4)
  })
})

describe('RecordList · копирование пароля из строки (Закон №1)', () => {
  /** Подменить буфер обмена и вернуть журнал того, что в него легло. */
  function stubClipboard(): string[] {
    const writes: string[] = []
    vi.stubGlobal('navigator', {
      clipboard: {
        writeText: vi.fn(async (text: string) => {
          writes.push(text)
        }),
      },
    })
    return writes
  }

  it('кнопка есть только у выбранной строки', async () => {
    const { wrapper } = await mountList()

    expect(wrapper.findAll('.record-list__copy')).toHaveLength(0)

    const github = useRecordsStore().records.find((record) => record.service_name === 'GitHub')!
    useVaultUiStore().openRecord(github.record_id)
    await flushPromises()

    expect(wrapper.findAll('.record-list__copy')).toHaveLength(1)
  })

  it('кладёт пароль в буфер, не показав его на экране', async () => {
    const writes = stubClipboard()
    const { wrapper } = await mountList()

    const github = useRecordsStore().records.find((record) => record.service_name === 'GitHub')!
    useVaultUiStore().openRecord(github.record_id)
    await flushPromises()

    await wrapper.find('.record-list__copy').trigger('click')
    await flushPromises()

    // В буфер лёг настоящий секрет...
    expect(writes[writes.length - 1]).toBe('mock-github-pw')
    // ...а на экране его нет и не было: копирование не требует показа.
    expect(wrapper.html()).not.toContain('mock-github-pw')
    // И в сторе метаданных его тоже нет — там просто нет такого поля.
    expect(JSON.stringify(useRecordsStore().records)).not.toContain('mock-github-pw')
  })
})
