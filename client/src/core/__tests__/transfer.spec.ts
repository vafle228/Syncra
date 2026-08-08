import { beforeEach, describe, expect, it } from 'vitest'

import type { ImportOptions } from '../contract'
import { createMockCoreClient, MOCK_MASTER_PASSWORD, type MockCoreClient } from '../mock'

/**
 * Экспорт, импорт и бэкап на уровне ядра (F12, §6.2).
 *
 * Главное, что здесь проверяется, — граница. Ни один пароль не пересекает IPC
 * ни в одну сторону: файл собирает и разбирает ядро, наружу идут путь, счётчики
 * и статусы строк. Всё остальное (подтверждения, предупреждения) — дело экрана.
 */

let core: MockCoreClient

/** Настройки мастера импорта. Слой ядра про них ничего не знает — их шлёт UI. */
const DEFAULT_IMPORT_OPTIONS: ImportOptions = { skip_duplicates: true, flag_reused: true }

beforeEach(() => {
  core = createMockCoreClient({ latencyMs: 0, startUnlocked: true })
})

/** Сид-секреты, которых не должно быть видно ни в одном ответе. */
function containsSecrets(value: unknown): boolean {
  const text = JSON.stringify(value)
  return /mock-[a-z]+-pw|MOCKTOTPSECRET|Recovery codes|mock-import-/.test(text)
}

describe('экспорт', () => {
  it('требует мастер-пароль, даже когда хранилище открыто', async () => {
    await expect(core.exportCsv('не тот пароль')).rejects.toMatchObject({
      code: 'INVALID_MASTER_PASSWORD',
    })
    await expect(core.exportBackup('не тот пароль')).rejects.toMatchObject({
      code: 'INVALID_MASTER_PASSWORD',
    })
  })

  it('отдаёт след файла, а не его содержимое (ЗАКОН №1)', async () => {
    const csv = await core.exportCsv(MOCK_MASTER_PASSWORD)

    expect(csv.record_count).toBe((await core.listRecords()).length)
    expect(csv.encrypted).toBe(false)
    expect(csv.path).toContain(csv.file_name)
    // Ни паролей, ни заметок, ни ключей TOTP — файла тут нет вообще.
    expect(containsSecrets(csv)).toBe(false)
    expect(Object.keys(csv).sort()).toEqual([
      'created_at',
      'encrypted',
      'file_name',
      'path',
      'record_count',
      'size_bytes',
    ])
  })

  it('помечает бэкап зашифрованным, а CSV — нет', async () => {
    expect((await core.exportBackup(MOCK_MASTER_PASSWORD)).encrypted).toBe(true)
    expect((await core.exportCsv(MOCK_MASTER_PASSWORD)).encrypted).toBe(false)
  })

  it('удаляет только тот файл, который создало само', async () => {
    const csv = await core.exportCsv(MOCK_MASTER_PASSWORD)

    await expect(core.deleteExport('~/Documents/чужой.txt')).rejects.toMatchObject({
      code: 'NOT_FOUND',
    })
    await expect(core.deleteExport(csv.path)).resolves.toBeUndefined()
    // Второй раз удалять уже нечего.
    await expect(core.deleteExport(csv.path)).rejects.toMatchObject({ code: 'NOT_FOUND' })
  })

  it('не работает на заблокированном хранилище', async () => {
    await core.lock()

    await expect(core.exportCsv(MOCK_MASTER_PASSWORD)).rejects.toMatchObject({ code: 'LOCKED' })
    await expect(core.exportBackup(MOCK_MASTER_PASSWORD)).rejects.toMatchObject({ code: 'LOCKED' })
  })
})

describe('импорт', () => {
  it('показывает, что попадёт внутрь, — без единого пароля (ЗАКОН №1)', async () => {
    const preview = await core.beginImport('chrome')

    expect(preview).not.toBeNull()
    expect(preview!.total_rows).toBeGreaterThan(0)
    expect(preview!.rows.length).toBeGreaterThan(0)
    expect(containsSecrets(preview)).toBe(false)
    // В строке предпросмотра ровно три поля, и «пароль» не одно из них.
    expect(Object.keys(preview!.rows[0]!).sort()).toEqual(['login', 'site', 'status'])
  })

  it('узнаёт дубликат по адресу и логину, а не по имени сервиса', async () => {
    const preview = await core.beginImport('chrome')
    const github = preview!.rows.find((row) => row.site === 'github.com')

    // `github.com / demo-user` уже есть в хранилище.
    expect(github?.status).toBe('duplicate')
    expect(preview!.duplicate_count).toBe(1)
    expect(preview!.no_password_count).toBe(1)
  })

  it('возвращает null, когда окно выбора файла закрыли', async () => {
    core.control.cancelNextFilePick()

    await expect(core.beginImport('chrome')).resolves.toBeNull()
  })

  it('заводит записи в отдельной секции и пропускает дубликаты', async () => {
    const before = (await core.listRecords()).length
    const preview = await core.beginImport('chrome')

    const result = await core.commitImport(preview!.session_id, DEFAULT_IMPORT_OPTIONS)

    expect(result.imported).toBe(preview!.new_count)
    expect(result.skipped).toBe(preview!.duplicate_count)
    expect(result.vault.name).toBe(preview!.target_vault_name)
    expect(result.source_file_deleted).toBe(true)

    const after = await core.listRecords()
    expect(after.length).toBe(before + result.imported)
    // Строка без пароля записью не становится: пустой пароль притворялся бы рабочим.
    expect(after.some((record) => record.service_name === 'Старый-форум')).toBe(false)
    // Всё легло в новую секцию, а не вперемешку со своим.
    expect(after.filter((record) => record.vault_id === result.vault.vault_id)).toHaveLength(
      result.imported,
    )
  })

  it('заводит дубликат вторым аккаунтом, если попросили не пропускать (§4.4)', async () => {
    const preview = await core.beginImport('chrome')

    const result = await core.commitImport(preview!.session_id, {
      ...DEFAULT_IMPORT_OPTIONS,
      skip_duplicates: false,
    })

    expect(result.skipped).toBe(0)
    expect(result.imported).toBe(preview!.new_count + preview!.duplicate_count)
  })

  it('считает повторяющиеся пароли только по просьбе', async () => {
    const first = await core.beginImport('chrome')
    const counted = await core.commitImport(first!.session_id, DEFAULT_IMPORT_OPTIONS)
    expect(counted.reused_passwords).toBeGreaterThan(0)

    const second = await core.beginImport('chrome')
    const silent = await core.commitImport(second!.session_id, {
      ...DEFAULT_IMPORT_OPTIONS,
      flag_reused: false,
    })
    expect(silent.reused_passwords).toBe(0)
  })

  it('переносит заметки и ключи TOTP там, где источник их отдаёт', async () => {
    const preview = await core.beginImport('1password')
    const result = await core.commitImport(preview!.session_id, DEFAULT_IMPORT_OPTIONS)

    const stripe = (await core.listRecords()).find((record) => record.service_name === 'Stripe')
    expect(result.imported).toBeGreaterThan(0)
    expect(stripe?.has_notes).toBe(true)
    expect(stripe?.has_totp).toBe(true)
  })

  it('забывает разобранный файл по отмене', async () => {
    const preview = await core.beginImport('chrome')

    await core.cancelImport(preview!.session_id)
    // Идемпотентно: по «Отмене» можно нажать дважды.
    await expect(core.cancelImport(preview!.session_id)).resolves.toBeUndefined()
    await expect(
      core.commitImport(preview!.session_id, DEFAULT_IMPORT_OPTIONS),
    ).rejects.toMatchObject({ code: 'NOT_FOUND' })
  })

  it('забывает разобранный файл вместе с замком: это чужие пароли', async () => {
    const preview = await core.beginImport('chrome')

    core.control.forceLock('timeout')
    await core.unlock(MOCK_MASTER_PASSWORD)

    await expect(
      core.commitImport(preview!.session_id, DEFAULT_IMPORT_OPTIONS),
    ).rejects.toMatchObject({ code: 'NOT_FOUND' })
  })
})

describe('восстановление из бэкапа (§6.3)', () => {
  it('не принимается поверх живого хранилища', async () => {
    await expect(core.restoreBackup(MOCK_MASTER_PASSWORD)).rejects.toMatchObject({
      code: 'ALREADY_INITIALIZED',
    })
  })

  it('открывает файл его собственным паролем и сразу разблокирует', async () => {
    const fresh = createMockCoreClient({ latencyMs: 0, initialized: false })

    await expect(fresh.restoreBackup('чужой пароль')).rejects.toMatchObject({
      code: 'INVALID_MASTER_PASSWORD',
    })

    const result = await fresh.restoreBackup(MOCK_MASTER_PASSWORD)

    expect(result).not.toBeNull()
    expect(result!.records).toBe((await fresh.listRecords()).length)
    expect(result!.vaults).toBe((await fresh.listVaults()).length)
    expect(fresh.control.isUnlocked()).toBe(true)
    expect(containsSecrets(result)).toBe(false)
  })

  it('возвращает null, когда окно выбора файла закрыли', async () => {
    const fresh = createMockCoreClient({ latencyMs: 0, initialized: false })
    fresh.control.cancelNextFilePick()

    await expect(fresh.restoreBackup(MOCK_MASTER_PASSWORD)).resolves.toBeNull()
    expect(fresh.control.isInitialized()).toBe(false)
  })
})
