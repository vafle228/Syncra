import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'

import App from '@/App.vue'
import { setCoreClient } from '@/core/ipc'
import {
  createMockCoreClient,
  MOCK_DEVICE_PHONE,
  MOCK_RECORD_GITHUB,
  MOCK_VAULT_WORK,
  type MockCoreClient,
} from '@/core/mock'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useVaultStore } from '@/stores/useVaultStore'
import { mountWithRouter, waitForRoute } from '@/test/mountWithRouter'

/**
 * Оболочка открытого хранилища (F13): сайдбар, средняя панель и правая панель
 * в одном окне.
 *
 * Здесь монтируется ВЕСЬ `App`, а не `VaultShell`. Причина техническая и важная:
 * `RouterView` берёт свою глубину из инъекции, поэтому у компонента,
 * смонтированного напрямую, вложенный `RouterView` отрисовал бы `matched[0]` —
 * то есть саму оболочку — и ушёл бы в рекурсию. Через `App` глубина настоящая:
 * 0 — оболочка, 1 — дочерний экран.
 *
 * Из этого следует и польза: сквозные сценарии «клик по строке слева → карточка
 * справа» проверяются здесь, потому что панели — сиблинги и по отдельности этот
 * путь не виден.
 */

afterEach(() => {
  setCoreClient(null)
})

async function mountShell(core?: MockCoreClient, path = '/') {
  return mountWithRouter(App, { core, path })
}

/** Найти строку сайдбара по имени секции. */
function section(wrapper: VueWrapper, name: string) {
  const found = wrapper
    .findAll('.sections__item')
    .find((node) => node.find('.sections__name').text() === name)
  if (!found) throw new Error(`Секция «${name}» не найдена в сайдбаре`)
  return found
}

describe('VaultShell · раскладка', () => {
  it('на главном экране показывает все три панели', async () => {
    const { wrapper } = await mountShell()

    expect(wrapper.find('[data-test="sidebar"]').exists()).toBe(true)
    expect(wrapper.find('[data-test="list-pane"]').exists()).toBe(true)
    expect(wrapper.find('[data-test="right-pane"]').exists()).toBe(true)
  })

  it('на прочих экранах список записей уходит, а сайдбар остаётся', async () => {
    // Это и есть смысл оболочки: «Настройки» — правая панель того же окна, а не
    // отдельная страница, с которой надо возвращаться.
    const { wrapper, router } = await mountShell(undefined, '/settings')
    expect(router.currentRoute.value.name).toBe('settings')

    expect(wrapper.find('[data-test="sidebar"]').exists()).toBe(true)
    expect(wrapper.find('[data-test="list-pane"]').exists()).toBe(false)
    expect(wrapper.text()).toContain('Настройки')
  })

  it('в шапке экрана нет ссылки «назад»: возвращаться неоткуда', async () => {
    const { wrapper } = await mountShell(undefined, '/devices')

    // Именно в шапке: контекстное «К паролям» в панели синхронизации — это
    // призыв разобрать конфликт, а не навигация назад, и оно остаётся.
    expect(wrapper.find('.devices__header').text()).not.toContain('К паролям')
    expect(wrapper.find('[data-test="sidebar"]').exists()).toBe(true)
  })

  it('поднимает данные сеанса один раз, а не на каждый заход на главный экран', async () => {
    // До F13 загрузка висела на главном экране, и возврат из настроек
    // перезапускал её — список моргал скелетом на ровном месте.
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    const { router } = await mountShell(core)

    const listRecords = vi.spyOn(core, 'listRecords')

    await router.push({ name: 'settings' })
    await flushPromises()
    await router.push({ name: 'home' })
    await flushPromises()

    // Оболочка не размонтировалась — значит и список не перезагружался.
    expect(listRecords).not.toHaveBeenCalled()
  })
})

describe('VaultShell · сайдбар секций (F7)', () => {
  it('показывает секции ядра со счётчиками и пометкой локальной', async () => {
    const { wrapper } = await mountShell()

    expect(section(wrapper, 'Все записи').text()).toContain('4')
    expect(section(wrapper, 'Личное').text()).toContain('3')
    expect(section(wrapper, 'Рабочее').text()).toContain('1')
    // «Рабочее» не синхронизируется — это видно там же, где её открывают.
    expect(section(wrapper, 'Рабочее').text()).toContain('локально')
    expect(section(wrapper, 'Личное').text()).not.toContain('локально')

    wrapper.unmount()
  })

  it('фильтрует список по выбранной секции', async () => {
    const { wrapper } = await mountShell()

    await section(wrapper, 'Рабочее').trigger('click')

    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(1)
    expect(wrapper.text()).toContain('work.demo@syncra.example')
    expect(wrapper.text()).not.toContain('demo-user')

    await section(wrapper, 'Все записи').trigger('click')
    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(4)

    wrapper.unmount()
  })

  it('ведёт на управление секциями, не теряя сайдбар', async () => {
    const { wrapper, router } = await mountShell()

    await wrapper.find('.sections__manage').trigger('click')
    await waitForRoute(router, 'sections')

    expect(wrapper.find('[data-test="sidebar"]').exists()).toBe(true)
    expect(wrapper.find('[data-test="list-pane"]').exists()).toBe(false)
  })

  it('считает доверенные устройства в нав-строке', async () => {
    const { wrapper } = await mountShell()

    const devices = wrapper
      .findAll('.vault-sidebar__link')
      .find((node) => node.text().includes('Устройства'))
    // Три доверенных устройства сида, включая это. Отозванных в счёте нет:
    // отозванное устройство доступа больше не имеет.
    expect(devices?.find('.vault-sidebar__count').text()).toBe('3')
  })
})

describe('VaultShell · состояние обмена в подвале (F10, F11)', () => {
  it('показывает состояние из ядра и отслеживает его смену', async () => {
    const core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
    const { wrapper } = await mountShell(core)

    // В сиде есть конфликт — он важнее всего остального (§ «Состояния»).
    expect(wrapper.find('.sidebar-sync__title').text()).toBe('1 конфликт')

    await useConflictsStore().resolve(MOCK_RECORD_GITHUB, 'local')
    await flushPromises()
    // Конфликта больше нет, но выбор ещё не уехал.
    expect(wrapper.find('.sidebar-sync__title').text()).toBe('1 изменение ждёт')

    core.control.peerFound(MOCK_DEVICE_PHONE)
    core.control.startSync(MOCK_DEVICE_PHONE)
    core.control.finishSync()
    await flushPromises()
    expect(wrapper.find('.sidebar-sync__title').text()).toBe('Синхронизировано')
  })
})

describe('VaultShell · сквозные пути между панелями', () => {
  it('открывает карточку по клику на строку списка', async () => {
    const { wrapper } = await mountShell()

    await wrapper.findAll('.record-list__row')[0]!.trigger('click')
    await flushPromises()

    const card = wrapper.find('.card')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('GitHub')
    // Открытая карточка не показывает секретов, пока их не попросили.
    expect(wrapper.html()).not.toMatch(/mock-[a-z]+-pw/)
  })

  it('заводит новую запись сквозным путём: форма → ядро → список', async () => {
    const { wrapper } = await mountShell()

    await wrapper.find('[data-test="toolbar-new"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('.form__title').text()).toBe('Новая запись')

    const inputs = wrapper.findAll<HTMLInputElement>('.form__grid .sy-input input')
    const set = async (index: number, value: string) => {
      const input = inputs[index]!
      input.element.value = value
      await input.trigger('input')
    }
    // Порядок полей в сетке: сервис, логин, адрес, метка.
    await set(0, 'Figma')
    await set(1, 'anna@studio.example')
    await set(2, 'https://figma.com')

    const password = wrapper.find<HTMLInputElement>('.form__secrets input[type="password"]')
    password.element.value = 'mock-figma-pw'
    await password.trigger('input')

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    // Запись появилась в списке, панель показывает уже её карточку.
    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(5)
    expect(wrapper.find('.card__title').text()).toBe('Figma')
    expect(wrapper.find('.record-list__count').text()).toContain('5 записей · 4 сервиса')
    expect(wrapper.html()).not.toContain('mock-figma-pw')
  })

  it('новая запись ложится в открытую секцию', async () => {
    const { wrapper } = await mountShell()
    await section(wrapper, 'Рабочее').trigger('click')

    await wrapper.find('[data-test="toolbar-new"]').trigger('click')
    await flushPromises()

    const inputs = wrapper.findAll<HTMLInputElement>('.form__grid .sy-input input')
    const set = async (index: number, value: string) => {
      const input = inputs[index]!
      input.element.value = value
      await input.trigger('input')
    }
    await set(0, 'Figma')
    await set(1, 'anna@studio.example')

    const password = wrapper.find<HTMLInputElement>('.form__secrets input[type="password"]')
    password.element.value = 'mock-figma-pw'
    await password.trigger('input')

    await wrapper.find('form').trigger('submit')
    await flushPromises()

    const created = useRecordsStore().records.find((record) => record.service_name === 'Figma')
    expect(created?.vault_id).toBe(MOCK_VAULT_WORK)

    wrapper.unmount()
  })

  it('после удаления запись уходит из списка, а панель — в пустое состояние', async () => {
    const { wrapper } = await mountShell()
    await wrapper.findAll('.record-list__row')[0]!.trigger('click')
    await flushPromises()

    await wrapper.find('.card__foot .sy-button--danger').trigger('click')
    await flushPromises()
    const confirm = [...document.body.querySelectorAll('.sy-modal__actions button')].find((node) =>
      node.textContent?.includes('Удалить запись'),
    ) as HTMLButtonElement
    confirm.click()
    await flushPromises()

    expect(wrapper.findAll('[data-test="row"]')).toHaveLength(3)
    expect(wrapper.text()).not.toContain('demo-user')
    expect(wrapper.text()).toContain('Запись не выбрана')
  })

  it('уход с главного экрана закрывает открытую форму', async () => {
    const { wrapper, router } = await mountShell()

    await wrapper.find('[data-test="toolbar-new"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('.form__title').exists()).toBe(true)

    await router.push({ name: 'sections' })
    await flushPromises()
    await router.push({ name: 'home' })
    await flushPromises()

    expect(wrapper.find('.form__title').exists()).toBe(false)
    expect(wrapper.text()).toContain('Запись не выбрана')
  })
})

describe('VaultShell · блокировка', () => {
  it('запирает хранилище и уводит на экран входа', async () => {
    const { wrapper, router } = await mountShell()

    await wrapper.find('[data-test="lock"]').trigger('click')
    await flushPromises()

    expect(useVaultStore().status).toBe('locked')
    // Роутер настоящий, поэтому проверяем не вызов `push`, а то, что приложение
    // правда стоит на экране входа — и что хранитель этому не помешал.
    await waitForRoute(router, 'unlock')
    expect(wrapper.find('[data-test="sidebar"]').exists()).toBe(false)
  })

  it('за замком не остаётся ни метаданных, ни секций на экране', async () => {
    const { wrapper, router } = await mountShell()
    expect(wrapper.text()).toContain('GitHub')

    await wrapper.find('[data-test="lock"]').trigger('click')
    await waitForRoute(router, 'unlock')

    expect(wrapper.text()).not.toContain('GitHub')
    expect(wrapper.text()).not.toContain('demo-user')
    expect(useSectionsStore().vaults).toHaveLength(0)
  })
})
