import {
  createRouter,
  createWebHistory,
  type RouteRecordRaw,
  type Router,
  type RouterHistory,
} from 'vue-router'

import VaultShell from '@/components/shell/VaultShell.vue'
import { useVaultStore, type VaultLockStatus } from '@/stores/useVaultStore'
import VaultView from '@/views/VaultView.vue'

/**
 * Маршруты и замок (F3, F13).
 *
 * Состояние хранилища решает, что вообще можно открыть: пока оно не создано —
 * только онбординг, пока заблокировано — только экран входа. Проверка живёт
 * в одном хранителе, чтобы «объехать» её нельзя было ни ссылкой, ни back.
 *
 * Экраны открытого хранилища — ДЕТИ одного роута с оболочкой `VaultShell`: в
 * окне прототипа они не отдельные страницы, а правая панель, пока сайдбар стоит
 * на месте. Адреса при этом не изменились: `/sections`, `/devices`, `/settings`
 * остались там же, где были, и старые закладки живы.
 *
 * Родительский роут НАМЕРЕННО без имени. Из этого следует, что `to.name` в
 * хранителе — всегда имя дочернего экрана, и `routeForStatus` ниже работает
 * ровно так же, как до вложенности.
 */

declare module 'vue-router' {
  interface RouteMeta {
    /** Показывать ли средний столбец со списком записей. */
    listPane?: boolean
  }
}

export const routes = [
  {
    path: '/',
    component: VaultShell,
    children: [
      {
        path: '',
        name: 'home',
        component: VaultView,
        // Средний столбец со списком записей есть только здесь: на остальных
        // экранах его место занимает сам экран.
        meta: { listPane: true },
      },
      {
        path: 'settings',
        name: 'settings',
        // Экран настроек нужен не в каждом сеансе — грузится лениво (F6, §3.11).
        component: () => import('@/views/SettingsView.vue'),
      },
      {
        path: 'sections',
        name: 'sections',
        // Управление секциями — редкий экран, грузится лениво (F7, §4.2).
        component: () => import('@/views/SectionsView.vue'),
      },
      {
        path: 'devices',
        name: 'devices',
        // Сопряжение по QR — редкий экран, грузится лениво (F8, §2.2).
        component: () => import('@/views/DevicesView.vue'),
      },
    ],
  },
  {
    /*
     * Импорт, бэкап и CSV переехали во вкладку «Настройки → Данные» (F13).
     * Адрес остаётся живым редиректом, а не исчезает: закладки на него делались,
     * и упереться в «страница не найдена» вместо переезда — плохая замена.
     * `?tab=import|export` вкладка принимает как есть.
     */
    path: '/data',
    redirect: { name: 'settings', query: { tab: 'data' } },
  },
  {
    path: '/unlock',
    name: 'unlock',
    // Экраны входа грузятся лениво: в разблокированном сеансе они не нужны.
    component: () => import('@/views/UnlockView.vue'),
  },
  {
    path: '/setup',
    name: 'setup',
    component: () => import('@/views/SetupView.vue'),
  },
] satisfies readonly RouteRecordRaw[]

/** Куда пускать при данном состоянии хранилища. */
export function routeForStatus(
  status: VaultLockStatus,
  target: string | symbol | null | undefined,
): { name: string } | true {
  if (status === 'uninitialized') return target === 'setup' ? true : { name: 'setup' }
  if (status === 'unlocked') {
    return target === 'unlock' || target === 'setup' ? { name: 'home' } : true
  }
  // 'locked' и любое неизвестное состояние — закрыто. Ошибаемся в сторону замка.
  return target === 'unlock' ? true : { name: 'unlock' }
}

export function createAppRouter(history: RouterHistory): Router {
  const router = createRouter({ history, routes: [...routes] })

  router.beforeEach(async (to) => {
    const vault = useVaultStore()
    const status = await vault.ensureStatus()
    return routeForStatus(status, to.name)
  })

  return router
}

export default createAppRouter(createWebHistory(import.meta.env.BASE_URL))
