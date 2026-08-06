import { computed, readonly, ref, type Ref } from 'vue'

/**
 * Тема оформления (F2).
 *
 * `system` следует за настройкой ОС, `dark` / `light` фиксируют выбор.
 * Разрешённая тема всегда пишется в `data-theme` на <html> — CSS в
 * `tokens.css` знает только про конкретные значения, не про `system`.
 *
 * Выбор темы — не секрет, его можно держать в localStorage. Ничего кроме
 * названия темы сюда класть нельзя.
 */

export type ThemePreference = 'system' | 'dark' | 'light'
export type ResolvedTheme = 'dark' | 'light'

const STORAGE_KEY = 'syncra.theme'

/** Тёмная — основная тема продукта; на неё же падаем при любой неясности. */
const DEFAULT_PREFERENCE: ThemePreference = 'system'
const FALLBACK_THEME: ResolvedTheme = 'dark'

function isThemePreference(value: unknown): value is ThemePreference {
  return value === 'system' || value === 'dark' || value === 'light'
}

function readStored(): ThemePreference {
  try {
    const stored = globalThis.localStorage?.getItem(STORAGE_KEY)
    return isThemePreference(stored) ? stored : DEFAULT_PREFERENCE
  } catch {
    // Приватный режим / отключённое хранилище — не повод падать.
    return DEFAULT_PREFERENCE
  }
}

function writeStored(preference: ThemePreference): void {
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, preference)
  } catch {
    /* см. readStored */
  }
}

function systemTheme(): ResolvedTheme {
  const query = globalThis.matchMedia?.('(prefers-color-scheme: light)')
  return query?.matches === true ? 'light' : FALLBACK_THEME
}

// Состояние на модуль: тема одна на всё приложение, сколько бы компонентов
// её ни спросило.
const preference: Ref<ThemePreference> = ref(DEFAULT_PREFERENCE)
const systemResolved: Ref<ResolvedTheme> = ref(FALLBACK_THEME)
let initialized = false

const resolved = computed<ResolvedTheme>(() =>
  preference.value === 'system' ? systemResolved.value : preference.value,
)

function apply(): void {
  globalThis.document?.documentElement.setAttribute('data-theme', resolved.value)
}

function set(next: ThemePreference): void {
  preference.value = next
  writeStored(next)
  apply()
}

/**
 * Поднимает тему из хранилища и подписывается на смену системной.
 * Идемпотентна: вызывается из `main.ts`, повторные вызовы ничего не делают.
 */
export function initTheme(): void {
  if (initialized) return
  initialized = true

  preference.value = readStored()
  systemResolved.value = systemTheme()

  const query = globalThis.matchMedia?.('(prefers-color-scheme: light)')
  query?.addEventListener?.('change', (event) => {
    systemResolved.value = event.matches ? 'light' : 'dark'
    apply()
  })

  apply()
}

/** Сброс состояния модуля — только для тестов. */
export function resetTheme(): void {
  initialized = false
  preference.value = DEFAULT_PREFERENCE
  systemResolved.value = FALLBACK_THEME
}

export function useTheme() {
  return {
    /** Что выбрал пользователь: `system` | `dark` | `light`. */
    preference: readonly(preference),
    /** Что реально применено к документу. */
    theme: resolved,
    setTheme: set,
    /** Переключатель в шапке: гоняет по кругу тёмная → светлая → системная. */
    cycleTheme(): void {
      const order: ThemePreference[] = ['dark', 'light', 'system']
      const index = order.indexOf(preference.value)
      set(order[(index + 1) % order.length] ?? 'system')
    },
  }
}
