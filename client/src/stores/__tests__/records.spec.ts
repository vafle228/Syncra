import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { setCoreClient } from '@/core/ipc'
import { createMockCoreClient, type MockCoreClient } from '@/core/mock'
import { useRecordsStore } from '../useRecordsStore'

let core: MockCoreClient

beforeEach(() => {
  setActivePinia(createPinia())
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
  setCoreClient(core)
})

afterEach(() => {
  setCoreClient(null)
})

describe('загрузка списка', () => {
  it('забирает метаданные у ядра и не показывает надгробия', async () => {
    const list = useRecordsStore()
    expect(list.loaded).toBe(false)

    await list.load()

    expect(list.loading).toBe(false)
    expect(list.loaded).toBe(true)
    // В сиде пять записей, одна из них — tombstone (§5.4).
    expect(list.total).toBe(4)
    expect(list.records.every((record) => record.deleted_at === null)).toBe(true)
    expect(list.error).toBeNull()
  })

  it('показывает сообщение ядра и оставляет список пустым при ошибке', async () => {
    core.control.failNext('INTERNAL', 'Ядро недоступно.')
    const list = useRecordsStore()

    await list.load()

    expect(list.error).toBe('Ядро недоступно.')
    expect(list.total).toBe(0)
    expect(list.loaded).toBe(false)
  })

  it('на заблокированном хранилище отдаёт сообщение ядра, а не падает', async () => {
    await core.lock()
    const list = useRecordsStore()

    await list.load()

    expect(list.error).toBe('Хранилище заблокировано.')
    expect(list.total).toBe(0)
  })
})

describe('поиск и группировка', () => {
  it('фильтрует по запросу и считает найденное', async () => {
    const list = useRecordsStore()
    await list.load()

    list.setQuery('google')

    expect(list.isSearching).toBe(true)
    expect(list.visible).toBe(2)
    expect(list.total).toBe(4)

    list.setQuery('google рабочий')
    expect(list.visible).toBe(1)

    list.clearQuery()
    expect(list.isSearching).toBe(false)
    expect(list.visible).toBe(4)
  })

  it('собирает два аккаунта Google в одну группу (§4.4)', async () => {
    const list = useRecordsStore()
    await list.load()

    const google = list.groups.find((group) => group.title === 'Google')

    expect(google?.records).toHaveLength(2)
    expect(google?.records.map((record) => record.login).sort()).toEqual([
      'personal.demo@gmail.com',
      'work.demo@syncra.example',
    ])
    // Четыре записи — три сервиса: Google, GitHub, Steam.
    expect(list.serviceCount).toBe(3)
  })

  it('ищет по адресу, которого нет в имени сервиса', async () => {
    const list = useRecordsStore()
    await list.load()

    list.setQuery('steampowered')

    expect(list.visible).toBe(1)
    expect(list.matched[0]?.service_name).toBe('Steam')
  })
})

describe('CRUD (F5)', () => {
  /** Черновик с заведомо узнаваемыми «секретами» — их будем искать в состоянии. */
  function draft() {
    return {
      service_name: 'Figma',
      urls: ['https://figma.com'],
      login: 'anna@studio.example',
      account_label: 'рабочий',
      password: 'mock-figma-pw',
      notes: 'mock-figma-note',
      totp_secret: 'MOCKTOTPFIGMA',
    }
  }

  it('создаёт запись, выбирает её и ставит на своё место в списке', async () => {
    const list = useRecordsStore()
    await list.load()

    const created = await list.create(draft())

    expect(list.total).toBe(5)
    expect(list.selectedId).toBe(created.record_id)
    expect(created.version).toBe(1)
    // Порядок ядра — по имени сервиса: Figma встаёт перед GitHub.
    expect(list.records.map((record) => record.service_name)).toEqual([
      'Figma',
      'GitHub',
      'Google',
      'Google',
      'Steam',
    ])
  })

  it('выводит флаги заполненности секретных полей из самих секретов', async () => {
    const list = useRecordsStore()
    await list.load()

    const withSecrets = await list.create(draft())
    expect(withSecrets.has_notes).toBe(true)
    expect(withSecrets.has_totp).toBe(true)

    const bare = await list.create({
      ...draft(),
      service_name: 'Bare',
      notes: null,
      totp_secret: null,
    })
    expect(bare.has_notes).toBe(false)
    expect(bare.has_totp).toBe(false)
  })

  it('изменяет метаданные, не трогая пароль', async () => {
    const list = useRecordsStore()
    await list.load()
    const github = list.records.find((record) => record.service_name === 'GitHub')!

    const updated = await list.update(github.record_id, { account_label: 'основной' })

    expect(updated.account_label).toBe('основной')
    expect(updated.version).toBe(github.version + 1)
    // Пароль не патчили — дата его смены не сдвинулась (§4.1).
    expect(updated.password_updated_at).toBe(github.password_updated_at)
    expect(list.records.find((record) => record.record_id === github.record_id)).toEqual(updated)

    // Секрет остался прежним: обновление метаданных его не переписало.
    const secrets = await core.getSecret(github.record_id)
    expect(secrets.password).toBe('mock-github-pw')
  })

  it('двигает password_updated_at только при смене пароля', async () => {
    const list = useRecordsStore()
    await list.load()
    const steam = list.records.find((record) => record.service_name === 'Steam')!

    const updated = await list.update(steam.record_id, { password: 'mock-steam-pw-2' })

    expect(updated.password_updated_at).not.toBe(steam.password_updated_at)
    expect((await core.getSecret(steam.record_id)).password).toBe('mock-steam-pw-2')
  })

  it('удаляет запись, снимает выбор и не оставляет её в живом списке', async () => {
    const list = useRecordsStore()
    await list.load()
    const steam = list.records.find((record) => record.service_name === 'Steam')!
    list.select(steam.record_id)

    await list.remove(steam.record_id)

    expect(list.total).toBe(3)
    expect(list.selectedId).toBeNull()
    expect(list.records.some((record) => record.record_id === steam.record_id)).toBe(false)

    // Ядро оставило надгробие (§5.4) — но его секретов больше нет.
    const tombstones = await core.listRecords({ include_deleted: true })
    const tombstone = tombstones.find((record) => record.record_id === steam.record_id)
    expect(tombstone?.deleted_at).not.toBeNull()
    await expect(core.getSecret(steam.record_id)).rejects.toMatchObject({ code: 'NOT_FOUND' })
  })

  it('пробрасывает ошибку ядра наружу, не гася список', async () => {
    const list = useRecordsStore()
    await list.load()

    core.control.failNext('VALIDATION', 'Поле «Логин» обязательно.')
    await expect(list.create(draft())).rejects.toMatchObject({ code: 'VALIDATION' })

    // Список остался на экране: ошибку показывает форма, а не пустая страница.
    expect(list.error).toBeNull()
    expect(list.total).toBe(4)
  })
})

describe('ЗАКОН №1', () => {
  it('черновик с секретами проходит транзитом и не оседает в состоянии', async () => {
    const list = useRecordsStore()
    await list.load()

    await list.create({
      service_name: 'Figma',
      urls: [],
      login: 'anna@studio.example',
      password: 'mock-figma-pw',
      notes: 'mock-figma-note',
      totp_secret: 'MOCKTOTPFIGMA',
    })
    const created = list.selectedId!
    await list.update(created, { password: 'mock-figma-pw-2', notes: 'mock-figma-note-2' })

    const snapshot = JSON.stringify(list.$state)

    expect(snapshot).not.toContain('mock-figma-pw')
    expect(snapshot).not.toContain('mock-figma-note')
    expect(snapshot).not.toContain('MOCKTOTPFIGMA')
    // При этом флаги-метаданные в состоянии есть — они не секрет.
    expect(list.selected?.has_totp).toBe(true)
  })

  it('в состоянии стора нет ни одного секрета', async () => {
    const list = useRecordsStore()
    await list.load()

    const snapshot = JSON.stringify(list.$state)

    // Все сид-секреты помечены `mock-…` / MOCKTOTP.
    expect(snapshot).not.toMatch(/mock-[a-z]+-pw/)
    expect(snapshot).not.toContain('MOCKTOTPSECRET')
    expect(snapshot).not.toContain('Recovery codes')
  })

  it('очищает список и снимает выбор по событию блокировки', async () => {
    const list = useRecordsStore()
    await list.load()
    list.setQuery('google')
    list.select(list.records[0]!.record_id)
    expect(list.total).toBe(4)

    // Заблокировать может не только пользователь: таймаут, сон системы.
    core.control.forceLock('timeout')

    expect(list.total).toBe(0)
    expect(list.query).toBe('')
    expect(list.loaded).toBe(false)
    expect(list.selectedId).toBeNull()
  })

  it('после dispose события больше не двигают состояние', async () => {
    const list = useRecordsStore()
    await list.load()

    list.dispose()
    core.control.forceLock('system')

    expect(list.total).toBe(4)
  })
})
