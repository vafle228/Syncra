import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, MOCK_MASTER_PASSWORD, type MockCoreClient } from '@/core/mock'
import BackupModal from '../BackupModal.vue'
import CsvExportModal from '../CsvExportModal.vue'
import ImportModal from '../ImportModal.vue'

/**
 * Импорт, бэкап и CSV-экспорт (F12, §3.10 макета) — с F13 три модалки вкладки
 * «Настройки → Данные», раньше один экран `/data`.
 *
 * Проверяем те же три обещания: предупреждение про CSV показывается и требует
 * подтверждения (§6.2 — формулировку не смягчаем), у опасного файла есть хвост
 * с кнопкой «Удалить файл сейчас», а импорт показывает, что попадёт внутрь, не
 * показывая при этом ни одного пароля.
 */

let core: MockCoreClient
let pinia: ReturnType<typeof createPinia>

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

/**
 * Модалка телепортируется в `body`; заглушка `teleport` оставляет её содержимое
 * внутри обёртки, чтобы утверждения читались как обычные.
 */
function modalMount() {
  return {
    props: { open: true },
    attachTo: document.body,
    global: { stubs: { teleport: true } },
  }
}

async function mountCsv() {
  const wrapper = mount(CsvExportModal, modalMount())
  await flushPromises()
  return wrapper
}

async function mountBackup() {
  const wrapper = mount(BackupModal, modalMount())
  await flushPromises()
  return wrapper
}

async function mountImport() {
  const wrapper = mount(ImportModal, modalMount())
  await flushPromises()
  return wrapper
}

/** Обёртка любой из трёх модалок: помощники ниже одинаковы для всех. */
type View = ReturnType<typeof mount>

function button(wrapper: View, text: string) {
  const found = wrapper.findAll('button').find((node) => node.text().includes(text))
  if (!found) throw new Error(`Кнопка «${text}» не найдена`)
  return found
}

/**
 * Пройти единственные ворота CSV-экспорта — мастер-пароль — и нажать кнопку.
 *
 * Ворот именно один: галочка и слово «ЭКСПОРТ» убраны, потому что три
 * подтверждения подряд учат нажимать не глядя. Пароль требует контракт.
 */
async function confirmCsv(wrapper: View, password = MOCK_MASTER_PASSWORD): Promise<void> {
  await wrapper.find('.csv input[type="password"]').setValue(password)
  await button(wrapper, 'Сохранить CSV').trigger('click')
  await flushPromises()
}

describe('CsvExportModal · экспорт в открытый текст (§6.2)', () => {
  it('предупреждает об открытом тексте до того, как файл создан', async () => {
    const wrapper = await mountCsv()

    // Формулировка §6.2 дословно, в янтарной полосе.
    expect(wrapper.text()).toContain('Файл не будет зашифрован')
    expect(wrapper.text()).toContain('читаются как обычный текст')
    expect(wrapper.text()).toContain('Удалите его сразу после переноса')
    expect(wrapper.find('.sy-modal__warning--warning').exists()).toBe(true)
    // Рамка диалога при этом нейтральная: красная здесь только кнопка.
    expect(wrapper.find('.sy-modal__dialog--tone-danger').exists()).toBe(false)

    wrapper.unmount()
  })

  it('ворота одни — мастер-пароль, и без него кнопка не работает', async () => {
    const wrapper = await mountCsv()

    // Ни галочки, ни слова-подтверждения: они были третьим и четвёртым «да».
    expect(wrapper.find('.csv__ack-box').exists()).toBe(false)
    expect(wrapper.find('.csv input[type="text"]').exists()).toBe(false)
    expect(button(wrapper, 'Сохранить CSV').attributes('disabled')).toBeDefined()

    await wrapper.find('.csv input[type="password"]').setValue(MOCK_MASTER_PASSWORD)
    expect(button(wrapper, 'Сохранить CSV').attributes('disabled')).toBeUndefined()

    // У поля мастер-пароля нет «Показать»: в макетах его нет ни у одного.
    expect(wrapper.find('.sy-input__reveal').exists()).toBe(false)

    wrapper.unmount()
  })

  it('не принимает чужой мастер-пароль и файла не создаёт', async () => {
    const wrapper = await mountCsv()

    await confirmCsv(wrapper, 'не тот пароль')

    expect(wrapper.text()).toContain('Неверный мастер-пароль.')
    expect(wrapper.text()).not.toContain('Открытый файл лежит на диске')

    wrapper.unmount()
  })

  it('после экспорта напоминает про файл и даёт его удалить', async () => {
    const wrapper = await mountCsv()

    await confirmCsv(wrapper)

    // Формулировку §6.2 не смягчаем: файл не зашифрован, удалите после использования.
    expect(wrapper.text()).toContain('Открытый файл лежит на диске')
    expect(wrapper.text()).toContain('Файл не зашифрован')
    expect(wrapper.text()).toContain('Удалите его сразу после использования')
    expect(wrapper.text()).toContain('syncra-plain-')

    await button(wrapper, 'Удалить файл сейчас').trigger('click')
    await flushPromises()

    expect(wrapper.text()).not.toContain('Открытый файл лежит на диске')
    // Пароль спрашивается заново: следующий такой файл — следующее решение.
    expect(button(wrapper, 'Сохранить CSV').attributes('disabled')).toBeDefined()

    wrapper.unmount()
  })
})

describe('BackupModal · зашифрованный бэкап', () => {
  it('сохраняет файл и показывает, где он лежит', async () => {
    const wrapper = await mountBackup()

    await wrapper.find('.backup input[type="password"]').setValue(MOCK_MASTER_PASSWORD)
    await button(wrapper, 'Сохранить бэкап').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Бэкап сохранён')
    expect(wrapper.text()).toContain('.syncra')
    expect(wrapper.text()).toContain('Сохранить ещё раз')
    // Наверх уходит след файла — карточка настроек показывает его у себя.
    expect(wrapper.emitted('done')).toHaveLength(1)

    wrapper.unmount()
  })

  it('заголовок один, а состав файла — сводка, а не столбик рамок', async () => {
    const wrapper = await mountBackup()

    // «Зашифрованный бэкап» рисует сам диалог; второй такой же в теле был
    // остатком спек-страницы.
    expect(wrapper.findAll('.sy-modal__title')).toHaveLength(1)
    expect(wrapper.text()).not.toContain('Спокойный вариант')
    expect(wrapper.findAll('.backup__spec')).toHaveLength(4)
    // Действие живёт в подвале диалога, как у остальных модалок.
    expect(wrapper.find('.sy-modal__actions').text()).toContain('Сохранить бэкап')

    wrapper.unmount()
  })

  it('говорит, что старый бэкап останется на старом пароле', async () => {
    const wrapper = await mountBackup()

    expect(wrapper.text()).toContain('старый бэкап продолжит открываться старым')

    wrapper.unmount()
  })
})

describe('ImportModal · импорт', () => {
  it('показывает, что попадёт внутрь, и ни одного пароля', async () => {
    const wrapper = await mountImport()

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Файл разобран')
    expect(wrapper.text()).toContain('пароли пока не показываем')
    expect(wrapper.text()).toContain('github.com')
    // Статусы строк видно, значений — нет.
    expect(wrapper.text()).toContain('дубликат')
    expect(wrapper.text()).toContain('без пароля')
    expect(wrapper.html()).not.toContain('mock-import-')

    wrapper.unmount()
  })

  it('переносит записи в отдельную секцию и сообщает, сколько приехало', async () => {
    const wrapper = await mountImport()
    const before = (await core.listRecords()).length

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()
    await button(wrapper, 'Импортировать').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('на месте')
    expect(wrapper.text()).toContain('Исходный файл удалён с диска')

    await button(wrapper, 'Показать импортированные').trigger('click')
    await flushPromises()

    expect((await core.listRecords()).length).toBeGreaterThan(before)
    // Куда идти дальше, решает экран настроек: модалка только сообщает, что и
    // сколько приехало. Фильтр, перезагрузку списка и баннер ставит он.
    const imported = wrapper.emitted('imported')
    expect(imported).toBeTruthy()
    expect(imported![0]![1]).toBeGreaterThan(0)

    wrapper.unmount()
  })

  it('отказ от импорта забывает разобранный файл', async () => {
    const wrapper = await mountImport()

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()
    await button(wrapper, 'Отмена').trigger('click')
    await flushPromises()

    expect(wrapper.text()).not.toContain('Файл разобран')
    expect(wrapper.find('.import__drop').exists()).toBe(true)

    wrapper.unmount()
  })

  it('молчит, когда окно выбора файла закрыли: это не ошибка', async () => {
    const wrapper = await mountImport()
    core.control.cancelNextFilePick()

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()

    expect(wrapper.find('.import__error').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Файл разобран')

    wrapper.unmount()
  })
})

describe('Модалки данных · ЗАКОН №1', () => {
  it('разобранный файл импорта не оседает в сторах', async () => {
    const wrapper = await mountImport()

    await wrapper.find('.import__drop').trigger('click')
    await flushPromises()

    const state = JSON.stringify(pinia.state.value)
    expect(state).not.toContain('session_id')

    wrapper.unmount()
  })

  it('ни путь к открытому файлу, ни мастер-пароль не оседают в сторах', async () => {
    const wrapper = await mountCsv()

    await confirmCsv(wrapper)

    const state = JSON.stringify(pinia.state.value)
    expect(state).not.toContain('syncra-plain-')
    expect(state).not.toContain(MOCK_MASTER_PASSWORD)

    wrapper.unmount()
  })

  it('не показывает ни одного секрета хранилища', async () => {
    const wrapper = await mountCsv()
    await confirmCsv(wrapper)

    expect(wrapper.html()).not.toMatch(/mock-[a-z]+-pw/)
    expect(wrapper.html()).not.toContain('MOCKTOTPSECRET')
    expect(wrapper.html()).not.toContain(MOCK_MASTER_PASSWORD)

    wrapper.unmount()
  })
})
