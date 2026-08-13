// @vitest-environment node

import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { describe, expect, it } from 'vitest'

/**
 * Страховка на неопределённые токены.
 *
 * `--sy-radius-md` полгода стоял в `RecordCard.vue` и `UnlockView.vue`, хотя в
 * `tokens.css` его нет: `var()` без значения и без фолбэка молча превращается в
 * пустоту, и поповер просто рисовался с прямыми углами. Тихая опечатка, которую
 * ни typecheck, ни lint не видят, — поэтому её ловит тест.
 *
 * Из проверки выпадают токены, чьё имя собирается в рантайме
 * (`var(--sy-vault-${color})`): после префикса там `$`, и регулярка их не берёт.
 */

const SRC = fileURLToPath(new URL('../..', import.meta.url))
const TOKENS = join(SRC, 'assets', 'tokens.css')

const SCANNED = ['.vue', '.ts', '.css']

function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return walk(path)
    return SCANNED.some((ext) => entry.name.endsWith(ext)) ? [path] : []
  })
}

/** Имена, объявленные в `tokens.css`. */
function definedTokens(): Set<string> {
  const css = readFileSync(TOKENS, 'utf8')
  return new Set(Array.from(css.matchAll(/^\s*(--sy-[a-z0-9-]+)\s*:/gm), (m) => m[1]!))
}

/** Все `var(--sy-…)` во всём дереве, вместе с файлом, где встретились. */
function usedTokens(): Map<string, string[]> {
  const used = new Map<string, string[]>()
  for (const file of walk(SRC)) {
    const text = readFileSync(file, 'utf8')
    for (const match of text.matchAll(/var\(\s*(--sy-[a-z0-9-]+?)\s*[),]/g)) {
      const name = match[1]!
      used.set(name, [...(used.get(name) ?? []), file])
    }
  }
  return used
}

describe('токены дизайн-системы', () => {
  it('каждый использованный --sy-* объявлен в tokens.css', () => {
    const defined = definedTokens()
    const undefinedUses = Array.from(usedTokens())
      .filter(([name]) => !defined.has(name))
      .map(([name, files]) => `${name} — ${files.map((f) => f.slice(SRC.length)).join(', ')}`)

    expect(undefinedUses).toEqual([])
  })

  it('находит хотя бы что-то — иначе проверка выше пройдёт вхолостую', () => {
    expect(definedTokens().size).toBeGreaterThan(50)
    expect(usedTokens().size).toBeGreaterThan(30)
  })
})
