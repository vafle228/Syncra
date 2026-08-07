import { createRouter, createWebHistory, type Router, type RouterHistory } from 'vue-router'

import { useVaultStore, type VaultLockStatus } from '@/stores/useVaultStore'
import HomeView from '@/views/HomeView.vue'

/**
 * Маршруты и замок (F3).
 *
 * Состояние хранилища решает, что вообще можно открыть: пока оно не создано —
 * только онбординг, пока заблокировано — только экран входа. Проверка живёт
 * в одном хранителе, чтобы «объехать» её нельзя было ни ссылкой, ни back.
 */

export const routes = [
  {
    path: '/',
    name: 'home',
    component: HomeView,
  },
  {
    path: '/settings',
    name: 'settings',
    // Экран настроек нужен не в каждом сеансе — грузится лениво (F6, §3.11).
    component: () => import('@/views/SettingsView.vue'),
  },
  {
    path: '/sections',
    name: 'sections',
    // Управление секциями — редкий экран, грузится лениво (F7, §4.2).
    component: () => import('@/views/SectionsView.vue'),
  },
  {
    path: '/devices',
    name: 'devices',
    // Сопряжение по QR — редкий экран, грузится лениво (F8, §2.2).
    component: () => import('@/views/DevicesView.vue'),
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
] as const

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
