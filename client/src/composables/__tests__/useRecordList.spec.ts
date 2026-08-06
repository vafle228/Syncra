import { describe, expect, it } from 'vitest'
import { ref } from 'vue'

import type { RecordMeta } from '@/core/contract'
import {
  groupRecords,
  matchesQuery,
  normalizeHost,
  parseQuery,
  useRecordList,
} from '../useRecordList'

let counter = 0

function record(partial: Partial<RecordMeta> = {}): RecordMeta {
  counter += 1
  const stamp = '2026-01-01T00:00:00.000Z'
  return {
    record_id: `rec-${counter}`,
    vault_id: 'vault-personal',
    service_name: 'Сервис',
    urls: [],
    login: 'user',
    account_label: null,
    version: 1,
    created_at: stamp,
    updated_at: stamp,
    password_updated_at: stamp,
    deleted_at: null,
    ...partial,
  }
}

function titles(records: RecordMeta[]): string[] {
  return records.map((item) => item.login)
}

describe('разбор строки поиска', () => {
  it('режет на термины, приводит к нижнему регистру и выкидывает пустое', () => {
    expect(parseQuery('  Google   Рабочий ')).toEqual(['google', 'рабочий'])
    expect(parseQuery('   ')).toEqual([])
    expect(parseQuery('')).toEqual([])
  })
})

describe('совпадение записи с запросом', () => {
  const google = record({
    service_name: 'Google',
    login: 'work.demo@syncra.example',
    account_label: 'Рабочий',
    urls: ['accounts.google.com', 'admin.google.com'],
  })

  it('ищет по имени сервиса, логину, метке аккаунта и адресам', () => {
    expect(matchesQuery(google, parseQuery('google'))).toBe(true)
    expect(matchesQuery(google, parseQuery('work.demo'))).toBe(true)
    expect(matchesQuery(google, parseQuery('рабочий'))).toBe(true)
    expect(matchesQuery(google, parseQuery('admin.google.com'))).toBe(true)
  })

  it('не зависит от регистра', () => {
    expect(matchesQuery(google, parseQuery('GOOGLE'))).toBe(true)
  })

  it('несколько слов работают как «И»', () => {
    expect(matchesQuery(google, parseQuery('google рабочий'))).toBe(true)
    expect(matchesQuery(google, parseQuery('google личный'))).toBe(false)
  })

  it('пустой запрос пропускает всё', () => {
    expect(matchesQuery(google, parseQuery(''))).toBe(true)
  })

  it('не находит того, чего в метаданных нет', () => {
    expect(matchesQuery(google, parseQuery('dropbox'))).toBe(false)
  })
})

describe('нормализация адреса', () => {
  it('снимает схему, www, порт, путь и точку на конце', () => {
    expect(normalizeHost('https://www.GitHub.com/login?next=1')).toBe('github.com')
    expect(normalizeHost('accounts.google.com.')).toBe('accounts.google.com')
    expect(normalizeHost('https://user@mail.google.com:8443/inbox')).toBe('mail.google.com')
  })

  it('возвращает null, если хоста нет', () => {
    expect(normalizeHost('   ')).toBeNull()
    expect(normalizeHost('/только/путь')).toBeNull()
  })
})

describe('группировка нескольких аккаунтов одного сервиса (§4.4)', () => {
  it('собирает вместе записи с общим хостом', () => {
    const personal = record({
      service_name: 'Google',
      login: 'personal@gmail.com',
      account_label: 'Личный',
      urls: ['accounts.google.com', 'mail.google.com'],
    })
    const work = record({
      service_name: 'Google',
      login: 'work@syncra.example',
      account_label: 'Рабочий',
      urls: ['accounts.google.com', 'admin.google.com'],
    })

    const groups = groupRecords([personal, work])

    expect(groups).toHaveLength(1)
    expect(groups[0]?.title).toBe('Google')
    expect(groups[0]?.records).toHaveLength(2)
  })

  it('разные сервисы остаются разными группами', () => {
    const groups = groupRecords([
      record({ service_name: 'GitHub', urls: ['github.com'] }),
      record({ service_name: 'Steam', urls: ['store.steampowered.com'] }),
    ])

    expect(groups.map((group) => group.title)).toEqual(['GitHub', 'Steam'])
    expect(groups.every((group) => group.records.length === 1)).toBe(true)
  })

  it('не склеивает разные сервисы на общем суффиксе домена', () => {
    // Без списка публичных суффиксов «отбросить поддомен» склеило бы эти два.
    // Недогруппировать безопаснее, чем угадать неверно (§4.4).
    const groups = groupRecords([
      record({ service_name: 'Foo', urls: ['foo.co.uk'] }),
      record({ service_name: 'Bar', urls: ['bar.co.uk'] }),
    ])

    expect(groups).toHaveLength(2)
  })

  it('связывает записи транзитивно через общие хосты', () => {
    const a = record({ service_name: 'Acme', login: 'a', urls: ['login.acme.test'] })
    const b = record({
      service_name: 'Acme',
      login: 'b',
      urls: ['login.acme.test', 'id.acme.test'],
    })
    const c = record({ service_name: 'Acme', login: 'c', urls: ['id.acme.test'] })

    const groups = groupRecords([a, c, b])

    expect(groups).toHaveLength(1)
    expect(titles(groups[0]?.records ?? []).sort()).toEqual(['a', 'b', 'c'])
  })

  it('записи без адресов группируются по имени сервиса', () => {
    const groups = groupRecords([
      record({ service_name: 'Netflix', login: 'a', urls: [] }),
      record({ service_name: 'Netflix', login: 'b', urls: [] }),
    ])

    expect(groups).toHaveLength(1)
    expect(groups[0]?.records).toHaveLength(2)
  })

  it('запись без адресов не приклеивается к одноимённой записи с адресом', () => {
    const groups = groupRecords([
      record({ service_name: 'Netflix', login: 'без адреса', urls: [] }),
      record({ service_name: 'Netflix', login: 'с адресом', urls: ['netflix.com'] }),
    ])

    expect(groups).toHaveLength(2)
  })

  it('сортирует группы по имени, а внутри — по метке и логину', () => {
    const groups = groupRecords([
      record({ service_name: 'Steam', urls: ['steamcommunity.com'] }),
      record({
        service_name: 'Google',
        login: 'work@syncra.example',
        account_label: 'Рабочий',
        urls: ['accounts.google.com'],
      }),
      record({
        service_name: 'Google',
        login: 'personal@gmail.com',
        account_label: 'Личный',
        urls: ['accounts.google.com'],
      }),
    ])

    expect(groups.map((group) => group.title)).toEqual(['Google', 'Steam'])
    expect(titles(groups[0]?.records ?? [])).toEqual(['personal@gmail.com', 'work@syncra.example'])
  })

  it('пустой список даёт пустой результат', () => {
    expect(groupRecords([])).toEqual([])
  })
})

describe('реактивный вид на список', () => {
  it('сначала фильтрует запросом, потом группирует уцелевшее', () => {
    const records = ref([
      record({
        service_name: 'Google',
        login: 'personal@gmail.com',
        account_label: 'Личный',
        urls: ['accounts.google.com'],
      }),
      record({
        service_name: 'Google',
        login: 'work@syncra.example',
        account_label: 'Рабочий',
        urls: ['accounts.google.com'],
      }),
      record({ service_name: 'GitHub', login: 'demo-user', urls: ['github.com'] }),
    ])
    const query = ref('')

    const list = useRecordList(records, query)

    expect(list.isSearching.value).toBe(false)
    expect(list.groups.value.map((group) => group.title)).toEqual(['GitHub', 'Google'])
    expect(list.groups.value.find((group) => group.title === 'Google')?.records).toHaveLength(2)

    query.value = 'рабочий'

    expect(list.isSearching.value).toBe(true)
    expect(list.matched.value).toHaveLength(1)
    // Совпал один аккаунт — группа показывает только его.
    expect(list.groups.value).toHaveLength(1)
    expect(list.groups.value[0]?.records).toHaveLength(1)
  })
})
