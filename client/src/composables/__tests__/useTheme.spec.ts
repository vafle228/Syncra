import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { initTheme, resetTheme, useTheme } from '../useTheme'

const STORAGE_KEY = 'syncra.theme'

/** jsdom не реализует matchMedia — подсовываем управляемую заглушку. */
function stubMatchMedia(prefersLight: boolean): void {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({
      matches: prefersLight,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  )
}

beforeEach(() => {
  resetTheme()
  localStorage.clear()
  document.documentElement.removeAttribute('data-theme')
  stubMatchMedia(false)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('useTheme', () => {
  it('по умолчанию следует за системой и падает в тёмную', () => {
    initTheme()

    const { preference, theme } = useTheme()
    expect(preference.value).toBe('system')
    expect(theme.value).toBe('dark')
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
  })

  it('разрешает системную светлую тему в data-theme', () => {
    stubMatchMedia(true)
    initTheme()

    expect(useTheme().theme.value).toBe('light')
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
  })

  it('сохраняет явный выбор и применяет его к документу', () => {
    initTheme()
    const { setTheme, theme } = useTheme()

    setTheme('light')

    expect(theme.value).toBe('light')
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
    expect(localStorage.getItem(STORAGE_KEY)).toBe('light')
  })

  it('поднимает сохранённый выбор при следующем запуске', () => {
    localStorage.setItem(STORAGE_KEY, 'dark')
    stubMatchMedia(true)

    initTheme()

    expect(useTheme().preference.value).toBe('dark')
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
  })

  it('игнорирует мусор в хранилище', () => {
    localStorage.setItem(STORAGE_KEY, 'неон')
    initTheme()

    expect(useTheme().preference.value).toBe('system')
  })

  it('гоняет переключатель по кругу тёмная → светлая → системная', () => {
    initTheme()
    const { preference, cycleTheme, setTheme } = useTheme()

    setTheme('dark')
    cycleTheme()
    expect(preference.value).toBe('light')
    cycleTheme()
    expect(preference.value).toBe('system')
    cycleTheme()
    expect(preference.value).toBe('dark')
  })
})
