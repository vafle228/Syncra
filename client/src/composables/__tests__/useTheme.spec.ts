import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { initTheme, resetTheme, useTheme } from '../useTheme'

const STORAGE_KEY = 'syncra.theme'
const ACCENT_KEY = 'syncra.accent'

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
  document.documentElement.removeAttribute('data-accent')
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

  it('умолчание акцента — мята, и атрибута у неё нет', () => {
    initTheme()

    expect(useTheme().accent.value).toBe('mint')
    // У мяты палитра лежит в `:root`: лишний атрибут потребовал бы её копии.
    expect(document.documentElement.hasAttribute('data-accent')).toBe(false)
  })

  it('акцент ставит data-accent и переживает перезапуск', () => {
    initTheme()
    useTheme().setAccent('amber')

    expect(document.documentElement.getAttribute('data-accent')).toBe('amber')
    expect(localStorage.getItem(ACCENT_KEY)).toBe('amber')

    resetTheme()
    initTheme()
    expect(useTheme().accent.value).toBe('amber')
    expect(document.documentElement.getAttribute('data-accent')).toBe('amber')
  })

  it('возврат к мяте снимает атрибут, а не пишет его пустым', () => {
    initTheme()
    const { setAccent } = useTheme()

    setAccent('indigo')
    setAccent('mint')

    expect(document.documentElement.hasAttribute('data-accent')).toBe(false)
  })

  it('незнакомый акцент в хранилище игнорируется', () => {
    localStorage.setItem(ACCENT_KEY, 'бирюза')
    initTheme()

    expect(useTheme().accent.value).toBe('mint')
  })

  it('тема и акцент — единственное, что фронт кладёт в localStorage', () => {
    initTheme()
    const { setTheme, setAccent } = useTheme()

    setTheme('light')
    setAccent('cyan')

    expect(Object.keys(localStorage).sort()).toEqual([ACCENT_KEY, STORAGE_KEY])
  })
})
