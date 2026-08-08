import {
  flushPromises,
  mount,
  type ComponentMountingOptions,
  type VueWrapper,
} from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import type { Component } from 'vue'
import { createMemoryHistory, type Router } from 'vue-router'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'
import { createAppRouter } from '@/router'

/**
 * Обвязка для тестов экранов: НАСТОЯЩИЙ роутер, мок-ядро и свежая Pinia.
 *
 * Зачем настоящий роутер, а не `vi.mock('vue-router')`: заглушка отдаёт только
 * `useRouter().push`, а экраны Syncra живут внутри вложенных роутов — им нужны
 * `RouterView`, `RouterLink`, `useRoute()` и навигационный хранитель. Подменённый
 * модуль не даёт ни одного из них, поэтому «перешли на другой экран» приходилось
 * проверять по вызову шпиона, а не по тому, куда приложение правда попало.
 *
 * Из этого следует главное свойство: хранитель здесь РАБОТАЕТ. Переход на экран,
 * закрытый замком, действительно упрётся в `unlock` — как в приложении. Поэтому
 * навигацию проверяем через `router.currentRoute.value.name`, а не через шпиона.
 *
 * Pinia создаётся здесь же и активируется до сборки роутера: хранитель на первом
 * же переходе спрашивает `useVaultStore()`, и без активной Pinia он упадёт.
 * Из этого следует правило: до `mountWithRouter()` сторы не трогать — стор,
 * взятый раньше, окажется из выброшенной Pinia. За ядром так же: `setCoreClient`
 * вызывается внутри, а `setCoreClient(null)` остаётся за `afterEach` теста.
 */

/** Тип заглушек — берётся у VTU, чтобы не разойтись с его сигнатурой. */
type Stubs = NonNullable<NonNullable<ComponentMountingOptions<unknown>['global']>['stubs']>

export interface RouterHarnessOptions {
  /**
   * Куда встать до монтирования. По умолчанию корень.
   *
   * Это ЗАПРОС, а не гарантия: хранитель может увести в другое место (например,
   * на `unlock`, если ядро отдало закрытое хранилище). Куда попали на самом деле
   * — смотреть в `router.currentRoute`.
   */
  path?: string
  /**
   * Готовое мок-ядро. По умолчанию — открытое хранилище без задержек: это то
   * состояние, в котором проверяется почти любой экран.
   */
  core?: MockCoreClient
  props?: Record<string, unknown>
  /**
   * Заглушки VTU. `RouterLink` и `RouterView` заглушать НЕ нужно — роутер
   * настоящий, и они разрешаются сами.
   */
  stubs?: Stubs
  /**
   * Дождаться ли ответов ядра перед возвратом. По умолчанию да.
   *
   * `false` нужен ровно там, где проверяется состояние ДО ответа — скелет
   * списка. С задержкой в ядре (`latencyMs`) тест тогда сам решает, сколько
   * ждать, и видит промежуточный кадр.
   */
  awaitCore?: boolean
}

export interface RouterHarness {
  wrapper: VueWrapper
  router: Router
  core: MockCoreClient
  pinia: Pinia
}

/**
 * Смонтировать компонент с настоящим роутером и дождаться ответов мок-ядра.
 *
 * Компонент монтируется НАПРЯМУЮ, а не через `RouterView`: так тест видит именно
 * тот экран, который проверяет, и не зависит от того, на каком роуте он висит.
 * Чтобы пройти сценарий переходами, монтируйте `App` и пользуйтесь `router`.
 */
export async function mountWithRouter(
  component: Component,
  options: RouterHarnessOptions = {},
): Promise<RouterHarness> {
  const pinia = createPinia()
  setActivePinia(pinia)

  const core = options.core ?? createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)

  const router = createAppRouter(createMemoryHistory())
  await router.push(options.path ?? '/')
  await router.isReady()

  const wrapper = mount(component, {
    props: options.props,
    global: {
      plugins: [pinia, router],
      stubs: options.stubs,
    },
  })

  // Экраны поднимают данные в `onMounted`, а мок-ядро отвечает через промисы:
  // без этого тест увидел бы скелет вместо списка.
  if (options.awaitCore !== false) await flushPromises()

  return { wrapper, router, core, pinia }
}

/** Сколько ждать перехода, прежде чем считать, что он не случится. */
const ROUTE_WAIT_MS = 1000

/**
 * Дождаться, пока роутер встанет на нужный роут.
 *
 * Нужно потому, что переход, начатый КОМПОНЕНТОМ, тесту недоступен: обещание
 * `router.push()` осталось внутри обработчика, и подождать его напрямую нельзя.
 *
 * Ждём по НАСТОЯЩИМ часам, а не пачкой `flushPromises()`. Причина: почти все
 * экраны — ленивые (`() => import(...)`), и первое разрешение такого импорта
 * делает реальную работу с диском. Микрозадачами её не подтолкнуть, поэтому
 * «прогоним flush двенадцать раз» отрабатывает на прогретом кеше модулей и
 * падает на холодном — то есть в зависимости от порядка тестов.
 *
 * Ограниченный опрос, а не пассивное ожидание: если переход не случился, тест
 * падает с внятным сообщением о том, где приложение осталось, а не на
 * непонятном `expect`.
 */
export async function waitForRoute(router: Router, name: string): Promise<void> {
  const deadline = Date.now() + ROUTE_WAIT_MS

  do {
    if (router.currentRoute.value.name === name) {
      // Роут сменился — дать компонентам новой страницы отрисоваться.
      await flushPromises()
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 4))
  } while (Date.now() < deadline)

  throw new Error(
    `Роутер не дошёл до «${name}» за ${ROUTE_WAIT_MS} мс: ` +
      `остался на «${String(router.currentRoute.value.name)}»`,
  )
}
