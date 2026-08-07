// @vitest-environment node
import { beforeEach, describe, expect, it } from 'vitest'

import type { CoreErrorCode } from '../contract'
import { VAULT_NAME_MAX_LENGTH } from '../contract'
import { isCoreError } from '../errors'
import {
  createMockCoreClient,
  MOCK_MASTER_PASSWORD,
  MOCK_VAULT_PERSONAL,
  MOCK_VAULT_WORK,
  type MockCoreClient,
} from '../mock'

/**
 * Секции фейк-ядра (F7, §4.2). Проверяем ровно те обещания, на которые
 * опирается интерфейс: секция по умолчанию есть всегда, удаление секции не
 * удаляет записи, флаг синхронизации сохраняется.
 */

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

let core: MockCoreClient

beforeEach(async () => {
  core = createMockCoreClient({ latencyMs: 0 })
  await core.unlock(MOCK_MASTER_PASSWORD)
})

describe('listVaults', () => {
  it('отдаёт секции сида с их флагами', async () => {
    const vaults = await core.listVaults()

    expect(vaults.map((vault) => vault.name)).toEqual(['Личное', 'Рабочее'])
    expect(vaults.filter((vault) => vault.is_default)).toHaveLength(1)
    // «Рабочее» в сиде локальное — иначе несинхронизируемую секцию негде увидеть.
    expect(vaults.find((vault) => vault.vault_id === MOCK_VAULT_WORK)?.sync).toBe(false)
  })

  it('за замком секций не показывает', async () => {
    const locked = createMockCoreClient({ latencyMs: 0 })

    await expectCoreError(locked.listVaults(), 'LOCKED')
    await expectCoreError(locked.createVault('Учёба', 'mint'), 'LOCKED')
    await expectCoreError(locked.setVaultSync(MOCK_VAULT_WORK, true), 'LOCKED')
  })

  it('возвращает копии: правка результата не меняет состояние ядра', async () => {
    const before = await core.listVaults()
    before[0]!.name = 'ПОДМЕНА'

    expect((await core.listVaults())[0]?.name).toBe('Личное')
  })
})

describe('создание и правка секции', () => {
  it('создаёт секцию синхронизируемой и не по умолчанию', async () => {
    const created = await core.createVault('  Учёба  ', 'mint')

    expect(created.name).toBe('Учёба')
    expect(created.color).toBe('mint')
    // Продукт про синхронизацию: молча оставить записи на одном устройстве
    // было бы сюрпризом.
    expect(created.sync).toBe(true)
    expect(created.is_default).toBe(false)
    expect(await core.listVaults()).toHaveLength(3)
  })

  it('не принимает пустое имя, слишком длинное имя и неизвестный цвет', async () => {
    await expectCoreError(core.createVault('   ', 'mint'), 'VALIDATION')
    await expectCoreError(
      core.createVault('x'.repeat(VAULT_NAME_MAX_LENGTH + 1), 'mint'),
      'VALIDATION',
    )
    await expectCoreError(
      core.createVault('Учёба', 'ультрафиолет' as unknown as 'mint'),
      'VALIDATION',
    )
  })

  it('переименовывает и перекрашивает, не трогая остального', async () => {
    const updated = await core.updateVault(MOCK_VAULT_WORK, { name: 'Работа' })

    expect(updated.name).toBe('Работа')
    expect(updated.color).toBe('amber')
    // Флаг синхронизации патчем не меняется — для него отдельная команда.
    expect(updated.sync).toBe(false)

    const recolored = await core.updateVault(MOCK_VAULT_WORK, { color: 'coral' })
    expect(recolored.name).toBe('Работа')
    expect(recolored.color).toBe('coral')
  })

  it('падает NOT_FOUND на неизвестной секции', async () => {
    await expectCoreError(core.updateVault('нет-такой', { name: 'x' }), 'NOT_FOUND')
    await expectCoreError(core.setVaultSync('нет-такой', true), 'NOT_FOUND')
    await expectCoreError(core.setDefaultVault('нет-такой'), 'NOT_FOUND')
    await expectCoreError(core.deleteVault('нет-такой'), 'NOT_FOUND')
  })
})

describe('синхронизация секции (§4.2)', () => {
  it('сохраняет флаг и отдаёт его следующим запросом', async () => {
    const off = await core.setVaultSync(MOCK_VAULT_PERSONAL, false)
    expect(off.sync).toBe(false)

    const stored = (await core.listVaults()).find((v) => v.vault_id === MOCK_VAULT_PERSONAL)
    expect(stored?.sync).toBe(false)

    const on = await core.setVaultSync(MOCK_VAULT_PERSONAL, true)
    expect(on.sync).toBe(true)
  })

  it('не трогает соседние секции', async () => {
    await core.setVaultSync(MOCK_VAULT_WORK, true)

    const personal = (await core.listVaults()).find((v) => v.vault_id === MOCK_VAULT_PERSONAL)
    expect(personal?.sync).toBe(true)
    expect(personal?.is_default).toBe(true)
  })
})

describe('секция по умолчанию', () => {
  it('помечена ровно одна — флаг снимается с прежней', async () => {
    const vaults = await core.setDefaultVault(MOCK_VAULT_WORK)

    expect(vaults.filter((vault) => vault.is_default).map((vault) => vault.vault_id)).toEqual([
      MOCK_VAULT_WORK,
    ])
  })

  it('туда ложится запись, созданная без секции', async () => {
    await core.setDefaultVault(MOCK_VAULT_WORK)

    const record = await core.createRecord({
      service_name: 'Figma',
      urls: [],
      login: 'anna',
      password: 'mock-figma-pw',
    })

    expect(record.vault_id).toBe(MOCK_VAULT_WORK)
  })

  it('не удаляется: новым записям нужно куда-то попадать', async () => {
    await expectCoreError(core.deleteVault(MOCK_VAULT_PERSONAL), 'VALIDATION')
    expect(await core.listVaults()).toHaveLength(2)
  })
})

describe('запись и её секция', () => {
  it('кладёт запись в указанную секцию', async () => {
    const record = await core.createRecord({
      vault_id: MOCK_VAULT_WORK,
      service_name: 'Jira',
      urls: [],
      login: 'd.demo',
      password: 'mock-jira-pw',
    })

    expect(record.vault_id).toBe(MOCK_VAULT_WORK)
    // В сиде у «Рабочего» одна живая запись и одно надгробие — в выдачу
    // попадают только живые (§5.4).
    expect(await core.listRecords({ vault_id: MOCK_VAULT_WORK })).toHaveLength(2)
  })

  it('не заводит запись в несуществующей секции', async () => {
    await expectCoreError(
      core.createRecord({
        vault_id: 'нет-такой',
        service_name: 'Figma',
        urls: [],
        login: 'anna',
        password: 'mock-figma-pw',
      }),
      'NOT_FOUND',
    )
  })

  it('переносит запись между секциями патчем', async () => {
    const [github] = (await core.listRecords()).filter((r) => r.service_name === 'GitHub')

    const moved = await core.updateRecord(github!.record_id, { vault_id: MOCK_VAULT_WORK })

    expect(moved.vault_id).toBe(MOCK_VAULT_WORK)
    expect(moved.version).toBe(github!.version + 1)
    await expectCoreError(
      core.updateRecord(github!.record_id, { vault_id: 'нет-такой' }),
      'NOT_FOUND',
    )
  })
})

describe('удаление секции', () => {
  it('не удаляет записи: они переезжают в секцию по умолчанию', async () => {
    const before = await core.listRecords({ vault_id: MOCK_VAULT_WORK })
    expect(before.length).toBeGreaterThan(0)
    const totalBefore = (await core.listRecords()).length

    const vaults = await core.deleteVault(MOCK_VAULT_WORK)

    expect(vaults.map((vault) => vault.vault_id)).toEqual([MOCK_VAULT_PERSONAL])
    // Ни одна запись не пропала — они просто сменили секцию (§4.2).
    expect(await core.listRecords()).toHaveLength(totalBefore)
    expect(await core.listRecords({ vault_id: MOCK_VAULT_WORK })).toHaveLength(0)

    for (const record of before) {
      const moved = (await core.listRecords()).find((r) => r.record_id === record.record_id)
      expect(moved?.vault_id).toBe(MOCK_VAULT_PERSONAL)
      // Переезд — обычное изменение записи: на другом устройстве он должен
      // выглядеть как правка, а не как ничего.
      expect(moved?.version).toBe(record.version + 1)
    }
  })

  it('секреты переехавших записей остаются при них', async () => {
    const work = (await core.listRecords({ vault_id: MOCK_VAULT_WORK }))[0]!

    await core.deleteVault(MOCK_VAULT_WORK)

    expect((await core.getSecret(work.record_id)).password).toBe('mock-google-work-pw')
  })
})

describe('жизненный цикл хранилища', () => {
  it('свежесозданное хранилище получает одну секцию по умолчанию', async () => {
    const fresh = createMockCoreClient({ latencyMs: 0, initialized: false })
    await fresh.initVault('рыжий трамвай у моста')

    const vaults = await fresh.listVaults()

    expect(vaults).toHaveLength(1)
    expect(vaults[0]?.is_default).toBe(true)
    expect(vaults[0]?.sync).toBe(true)
  })

  it('reset возвращает секции сида', async () => {
    await core.createVault('Учёба', 'mint')
    await core.setVaultSync(MOCK_VAULT_WORK, true)

    core.control.reset()
    await core.unlock(MOCK_MASTER_PASSWORD)

    const vaults = await core.listVaults()
    expect(vaults.map((vault) => vault.name)).toEqual(['Личное', 'Рабочее'])
    expect(vaults[1]?.sync).toBe(false)
  })
})
