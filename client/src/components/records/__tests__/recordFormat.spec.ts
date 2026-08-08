import { describe, expect, it } from 'vitest'

import { daysSince, formatDate, hostOf, passwordAgeWarning } from '../recordFormat'

const NOW = new Date('2026-08-06T12:00:00.000Z')

describe('formatDate', () => {
  it('показывает дату по-русски', () => {
    expect(formatDate('2026-01-09T17:44:12.000Z')).toContain('2026')
  })

  it('не падает на мусоре из ядра', () => {
    expect(formatDate('не-дата')).toBe('—')
    expect(daysSince('не-дата')).toBeNull()
  })
})

describe('passwordAgeWarning', () => {
  it('молчит про свежий пароль', () => {
    expect(passwordAgeWarning('2026-06-01T00:00:00.000Z', NOW)).toBeNull()
    // Ровно год без одного дня — ещё не повод отвлекать.
    expect(passwordAgeWarning('2025-08-10T00:00:00.000Z', NOW)).toBeNull()
  })

  it('предупреждает, когда счёт пошёл на годы', () => {
    expect(passwordAgeWarning('2025-01-01T00:00:00.000Z', NOW)).toBe(
      'Пароль не менялся больше года — стоит заменить.',
    )
    expect(passwordAgeWarning('2023-01-01T00:00:00.000Z', NOW)).toBe(
      'Пароль не менялся больше 3 лет — стоит заменить.',
    )
  })
})

describe('hostOf', () => {
  it('оставляет от адреса то, что человек узнаёт с одного взгляда', () => {
    expect(hostOf('https://admin.google.com/u/0/ac/users')).toBe('admin.google.com')
    expect(hostOf('http://localhost:5173/records')).toBe('localhost:5173')
    // Схемы в сиде нет — адрес всё равно должен читаться.
    expect(hostOf('store.steampowered.com')).toBe('store.steampowered.com')
  })

  it('на неразбираемой строке возвращает её саму, а не прочерк', () => {
    // Это то, что человек когда-то ввёл. Показывать вместо адреса «—» значило бы
    // делать вид, что поле пустое, когда оно заполнено.
    expect(hostOf('не адрес вовсе')).toBe('не адрес вовсе')
    expect(hostOf('   ')).toBe('')
  })
})
