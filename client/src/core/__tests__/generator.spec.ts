// @vitest-environment node
import { beforeEach, describe, expect, it } from 'vitest'

import type { CoreErrorCode, GeneratorProfile } from '../contract'
import { GENERATOR_LIMITS } from '../contract'
import { isCoreError } from '../errors'
import {
  createMockCoreClient,
  DEFAULT_GENERATOR_PROFILE,
  MOCK_MASTER_PASSWORD,
  type MockCoreClient,
} from '../mock'

/**
 * Генератор фейк-ядра (F6, §6.1).
 *
 * Проверяем ровно то, за что отвечает ядро: правила сохраняются, варианты
 * приходят пачкой, алфавит слушается профиля, а оценка стойкости считается по
 * тому же алфавиту, которым пароль собран.
 */

let core: MockCoreClient

function profile(overrides: Partial<GeneratorProfile> = {}): GeneratorProfile {
  return { ...DEFAULT_GENERATOR_PROFILE, ...overrides }
}

async function expectCoreError(promise: Promise<unknown>, code: CoreErrorCode): Promise<void> {
  let thrown: unknown = null
  let resolved = false

  try {
    await promise
    resolved = true
  } catch (error) {
    thrown = error
  }

  expect(resolved, `ожидалась ошибка ядра ${code}, но промис зарезолвился`).toBe(false)
  expect(isCoreError(thrown, code), `ожидалась ошибка ядра ${code}, получено: ${thrown}`).toBe(true)
}

beforeEach(async () => {
  core = createMockCoreClient({ latencyMs: 0 })
  await core.unlock(MOCK_MASTER_PASSWORD)
})

describe('профиль генерации', () => {
  it('отдаёт профиль по умолчанию, пока его не меняли', async () => {
    await expect(core.getGeneratorProfile()).resolves.toEqual(DEFAULT_GENERATOR_PROFILE)
  })

  it('сохраняет правила один раз и помнит их дальше (§6.1)', async () => {
    const next = profile({ mode: 'words', words: 5, separator: '.', append_number: true })

    await expect(core.saveGeneratorProfile(next)).resolves.toEqual(next)
    // Именно «настроил один раз»: следующий запрос отдаёт те же правила.
    await expect(core.getGeneratorProfile()).resolves.toEqual(next)
  })

  it('возвращает копию: правка ответа не меняет состояние ядра', async () => {
    const stored = await core.getGeneratorProfile()
    stored.length = 999

    await expect(core.getGeneratorProfile()).resolves.toEqual(DEFAULT_GENERATOR_PROFILE)
  })

  it('не принимает правила вне своих границ', async () => {
    await expectCoreError(
      core.saveGeneratorProfile(profile({ length: GENERATOR_LIMITS.length.max + 1 })),
      'VALIDATION',
    )
    await expectCoreError(
      core.saveGeneratorProfile(profile({ length: GENERATOR_LIMITS.length.min - 1 })),
      'VALIDATION',
    )
    await expectCoreError(
      core.saveGeneratorProfile(profile({ words: GENERATOR_LIMITS.words.max + 1 })),
      'VALIDATION',
    )
    await expectCoreError(
      core.saveGeneratorProfile(profile({ separator: ';' as GeneratorProfile['separator'] })),
      'VALIDATION',
    )
    await expectCoreError(
      core.saveGeneratorProfile(profile({ mode: 'диктовка' as GeneratorProfile['mode'] })),
      'VALIDATION',
    )

    // Ни одна отклонённая попытка не должна была подменить сохранённый профиль.
    await expect(core.getGeneratorProfile()).resolves.toEqual(DEFAULT_GENERATOR_PROFILE)
  })

  it('свежесозданное хранилище получает профиль по умолчанию', async () => {
    const fresh = createMockCoreClient({ latencyMs: 0, initialized: false })
    await fresh.initVault('достаточно-длинный-пароль')

    await expect(fresh.getGeneratorProfile()).resolves.toEqual(DEFAULT_GENERATOR_PROFILE)
  })
})

describe('генерация', () => {
  it('отдаёт ровно столько вариантов, сколько попросили (§6.1)', async () => {
    const { passwords } = await core.generatePasswords(5)

    expect(passwords).toHaveLength(5)
    expect(passwords.every((password) => password.length > 0)).toBe(true)
  })

  it('варианты различаются между собой и от запроса к запросу', async () => {
    const first = await core.generatePasswords(5)
    const second = await core.generatePasswords(5)

    expect(new Set(first.passwords).size).toBe(5)
    expect(first.passwords).not.toEqual(second.passwords)
  })

  it('слушается длины из профиля', async () => {
    await core.saveGeneratorProfile(profile({ mode: 'chars', length: 32 }))
    const { passwords } = await core.generatePasswords(3)

    expect(passwords.every((password) => password.length === 32)).toBe(true)
  })

  it('без цифр и символов оставляет одни буквы', async () => {
    await core.saveGeneratorProfile(profile({ mode: 'chars', digits: false, symbols: false }))
    const { passwords } = await core.generatePasswords(5)

    expect(passwords.every((password) => /^[a-zA-Z]+$/.test(password))).toBe(true)
  })

  it('исключает похожие символы, когда это включено', async () => {
    await core.saveGeneratorProfile(
      profile({ mode: 'chars', length: 40, avoid_ambiguous: true, digits: true, symbols: false }),
    )
    // Сорок символов на сорок вариантов: если бы `0`, `O`, `l`, `I` или `1`
    // оставались в алфавите, они бы почти наверняка попались.
    const { passwords } = await core.generatePasswords(10)

    expect(passwords.join('')).not.toMatch(/[loIO01]/)
  })

  it('в режиме слов собирает фразу выбранным разделителем', async () => {
    await core.saveGeneratorProfile(
      profile({ mode: 'words', words: 4, separator: '-', append_number: false }),
    )
    const { passwords } = await core.generatePasswords(3)

    for (const phrase of passwords) {
      const parts = phrase.split('-')
      expect(parts).toHaveLength(4)
      expect(parts.every((word) => /^[а-яё]+$/.test(word))).toBe(true)
    }
  })

  it('дописывает число в конец фразы, когда это включено', async () => {
    await core.saveGeneratorProfile(
      profile({ mode: 'words', words: 3, separator: '.', append_number: true }),
    )
    const { passwords } = await core.generatePasswords(3)

    for (const phrase of passwords) {
      const parts = phrase.split('.')
      expect(parts).toHaveLength(4)
      expect(parts[parts.length - 1]).toMatch(/^\d\d$/)
    }
  })

  it('разовый профиль не переписывает сохранённый', async () => {
    const once = profile({ mode: 'chars', length: 40 })
    const { passwords } = await core.generatePasswords(2, once)

    expect(passwords.every((password) => password.length === 40)).toBe(true)
    await expect(core.getGeneratorProfile()).resolves.toEqual(DEFAULT_GENERATOR_PROFILE)
  })

  it('проверяет разовый профиль так же строго, как сохраняемый', async () => {
    await expectCoreError(core.generatePasswords(1, profile({ length: 4 })), 'VALIDATION')
  })

  it('не отдаёт вариантов больше, чем разрешено', async () => {
    await expectCoreError(core.generatePasswords(GENERATOR_LIMITS.count.max + 1), 'VALIDATION')
    await expectCoreError(core.generatePasswords(0), 'VALIDATION')
  })
})

describe('оценка стойкости', () => {
  it('растёт с длиной и с богатством алфавита', async () => {
    const short = await core.generatePasswords(1, profile({ mode: 'chars', length: 12 }))
    const long = await core.generatePasswords(1, profile({ mode: 'chars', length: 24 }))
    const lettersOnly = await core.generatePasswords(
      1,
      profile({ mode: 'chars', length: 24, digits: false, symbols: false }),
    )

    expect(long.entropy_bits).toBeGreaterThan(short.entropy_bits)
    expect(long.entropy_bits).toBeGreaterThan(lettersOnly.entropy_bits)
  })

  it('считается по тому же словарю, которым собрана фраза', async () => {
    const three = await core.generatePasswords(1, profile({ mode: 'words', words: 3 }))
    const five = await core.generatePasswords(1, profile({ mode: 'words', words: 5 }))

    // Каждое слово добавляет одинаково — оценка линейна по числу слов.
    expect(five.entropy_bits).toBeGreaterThan(three.entropy_bits)
    expect(five.entropy_bits / three.entropy_bits).toBeCloseTo(5 / 3, 1)
  })
})

describe('замок', () => {
  it('не отдаёт ни правил, ни паролей на заблокированном хранилище', async () => {
    const locked = createMockCoreClient({ latencyMs: 0 })

    await expectCoreError(locked.getGeneratorProfile(), 'LOCKED')
    await expectCoreError(locked.saveGeneratorProfile(DEFAULT_GENERATOR_PROFILE), 'LOCKED')
    await expectCoreError(locked.generatePasswords(3), 'LOCKED')
  })

  it('reset возвращает профиль по умолчанию', async () => {
    await core.saveGeneratorProfile(profile({ mode: 'words', words: 7 }))

    core.control.reset()
    await core.unlock(MOCK_MASTER_PASSWORD)

    await expect(core.getGeneratorProfile()).resolves.toEqual(DEFAULT_GENERATOR_PROFILE)
  })
})
