import { computed, readonly, ref, type Ref } from 'vue'

/**
 * Оформление: тема и акцент (F2, F13).
 *
 * `system` следует за настройкой ОС, `dark` / `light` фиксируют выбор.
 * Разрешённая тема всегда пишется в `data-theme` на <html> — CSS в
 * `tokens.css` знает только про конкретные значения, не про `system`.
 *
 * Акцент устроен так же и живёт в `data-accent`. Умолчание — мята: у неё
 * атрибута нет вовсе, её палитра лежит в `:root`.
 *
 * Ни тема, ни акцент не секреты, их можно держать в localStorage. Это
 * единственные два ключа, которые фронт туда пишет, и ничего, кроме названий
 * темы и акцента, сюда класть нельзя.
 */

export type ThemePreference = 'system' | 'dark' | 'light'
export type ResolvedTheme = 'dark' | 'light'
/** Четыре палитры прототипа (`Прототип:2595-2600`). */
export type AccentPreference = 'mint' | 'cyan' | 'amber' | 'indigo'

export const ACCENTS: AccentPreference[] = ['mint', 'cyan', 'amber', 'indigo']

const STORAGE_KEY = 'syncra.theme'
const ACCENT_STORAGE_KEY = 'syncra.accent'

/** Тёмная — основная тема продукта; на неё же падаем при любой неясности. */
const DEFAULT_PREFERENCE: ThemePreference = 'system'
const FALLBACK_THEME: ResolvedTheme = 'dark'
const DEFAULT_ACCENT: AccentPreference = 'mint'

function isThemePreference(value: unknown): value is ThemePreference {
  return value === 'system' || value === 'dark' || value === 'light'
}

function isAccent(value: unknown): value is AccentPreference {
  return ACCENTS.includes(value as AccentPreference)
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

function readStoredAccent(): AccentPreference {
  try {
    const stored = globalThis.localStorage?.getItem(ACCENT_STORAGE_KEY)
    return isAccent(stored) ? stored : DEFAULT_ACCENT
  } catch {
    return DEFAULT_ACCENT
  }
}

function writeStoredAccent(value: AccentPreference): void {
  try {
    globalThis.localStorage?.setItem(ACCENT_STORAGE_KEY, value)
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
const accent: Ref<AccentPreference> = ref(DEFAULT_ACCENT)
const systemResolved: Ref<ResolvedTheme> = ref(FALLBACK_THEME)
let initialized = false

const resolved = computed<ResolvedTheme>(() =>
  preference.value === 'system' ? systemResolved.value : preference.value,
)

function apply(): void {
  const root = globalThis.document?.documentElement
  if (!root) return

  root.setAttribute('data-theme', resolved.value)
  // У мяты атрибута нет: её палитра — это `:root`, и лишний атрибут заставил бы
  // держать для умолчания второй, дублирующий блок токенов.
  if (accent.value === DEFAULT_ACCENT) root.removeAttribute('data-accent')
  else root.setAttribute('data-accent', accent.value)
}

function set(next: ThemePreference): void {
  preference.value = next
  writeStored(next)
  apply()
}

function setAccentValue(next: AccentPreference): void {
  accent.value = next
  writeStoredAccent(next)
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
  accent.value = readStoredAccent()
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
  accent.value = DEFAULT_ACCENT
  systemResolved.value = FALLBACK_THEME
}

export function useTheme() {
  return {
    /** Что выбрал пользователь: `system` | `dark` | `light`. */
    preference: readonly(preference),
    /** Что реально применено к документу. */
    theme: resolved,
    setTheme: set,
    /** Выбранная палитра акцента. */
    accent: readonly(accent),
    setAccent: setAccentValue,
  }
}
