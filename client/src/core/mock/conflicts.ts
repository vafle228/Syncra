import type {
  ConflictField,
  ConflictVersion,
  IsoDateTime,
  RecordConflict,
  RecordId,
  RecordMeta,
  RecordSecrets,
  VaultId,
} from '../contract'
import { secretFlags } from './seed'

/**
 * Конфликты версий в фейк-ядре (F11, §5.5).
 *
 * Здесь нет ни сети, ни протокола: настоящее ядро обнаруживает расхождение при
 * обмене диффом (§5.3), а фейк-ядро просто ДЕРЖИТ рядом с местной записью
 * вторую версию — «ту, что приехала». Этого достаточно, чтобы экран разрешения
 * конфликта был полноценным: у него есть обе стороны, обе даты и обе версии.
 */

/** Метаданные, по которым версии могут разойтись (кроме секретов). */
export interface ConflictMetaFields {
  vault_id: VaultId
  service_name: string
  urls: string[]
  login: string
  account_label: string | null
}

/**
 * Приехавшая версия записи. Живёт в фейк-ядре рядом с местной и наружу целиком
 * не отдаётся: секреты из неё уходят только через `get_conflict_secret`.
 */
export interface MockConflictEntry {
  record_id: RecordId
  raised_at: IsoDateTime
  /** Устройство, на котором сделали эту правку. */
  device_name: string
  version: number
  updated_at: IsoDateTime
  meta: ConflictMetaFields
  secrets: RecordSecrets
}

function sameUrls(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((url, index) => url === b[index])
}

/** `null` и пустая строка — одно и то же «поле не заполнено». */
function sameText(a: string | null, b: string | null): boolean {
  return (a ?? '') === (b ?? '')
}

/**
 * По каким полям версии расходятся.
 *
 * ЗАКОН №1: секреты сравниваются ЗДЕСЬ, внутри ядра, и наружу уходит только имя
 * поля. «Пароли различаются» — факт о записи; сам пароль для этого факта UI не
 * нужен и не выдаётся.
 */
export function conflictDiff(
  local: ConflictMetaFields,
  localSecrets: RecordSecrets,
  remote: ConflictMetaFields,
  remoteSecrets: RecordSecrets,
): ConflictField[] {
  const fields: ConflictField[] = []

  if (local.vault_id !== remote.vault_id) fields.push('vault_id')
  if (local.service_name !== remote.service_name) fields.push('service_name')
  if (!sameUrls(local.urls, remote.urls)) fields.push('urls')
  if (local.login !== remote.login) fields.push('login')
  if (!sameText(local.account_label, remote.account_label)) fields.push('account_label')

  if (localSecrets.password !== remoteSecrets.password) fields.push('password')
  if (!sameText(localSecrets.notes, remoteSecrets.notes)) fields.push('notes')
  if (!sameText(localSecrets.totp_secret, remoteSecrets.totp_secret)) fields.push('totp_secret')

  return fields
}

function remoteVersion(entry: MockConflictEntry): ConflictVersion {
  return {
    side: 'remote',
    device_name: entry.device_name,
    version: entry.version,
    updated_at: entry.updated_at,
    vault_id: entry.meta.vault_id,
    service_name: entry.meta.service_name,
    urls: [...entry.meta.urls],
    login: entry.meta.login,
    account_label: entry.meta.account_label,
    ...secretFlags(entry.secrets),
  }
}

function localVersion(meta: RecordMeta, deviceName: string): ConflictVersion {
  return {
    side: 'local',
    device_name: deviceName,
    version: meta.version,
    updated_at: meta.updated_at,
    vault_id: meta.vault_id,
    service_name: meta.service_name,
    urls: [...meta.urls],
    login: meta.login,
    account_label: meta.account_label,
    has_notes: meta.has_notes,
    has_totp: meta.has_totp,
  }
}

/**
 * Собрать конфликт для отдачи в UI.
 *
 * Местная сторона строится из ЖИВОЙ записи, а не из копии, снятой в момент
 * расхождения: пока конфликт ждёт решения, запись можно править, и показывать
 * человеку устаревший вариант его же данных было бы враньём.
 */
export function buildConflict(
  entry: MockConflictEntry,
  meta: RecordMeta,
  secrets: RecordSecrets,
  thisDeviceName: string,
): RecordConflict {
  return {
    record_id: entry.record_id,
    raised_at: entry.raised_at,
    local: localVersion(meta, thisDeviceName),
    remote: remoteVersion(entry),
    differing_fields: conflictDiff(meta, secrets, entry.meta, entry.secrets),
  }
}
