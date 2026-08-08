import type { ConflictField, ConflictVersion, RecordConflict, SecretField } from '@/core/contract'

/**
 * Подписи экрана разрешения конфликта (F11, §5.5). Чистые функции: ни команды
 * в ядро, ни секретного значения — только то, что уже пришло в `RecordConflict`.
 */

/** Порядок строк diff-а: сначала «что это за запись», потом секреты. */
export const CONFLICT_FIELD_ORDER: ConflictField[] = [
  'service_name',
  'login',
  'account_label',
  'urls',
  'vault_id',
  'password',
  'notes',
  'totp_secret',
]

const LABELS: Record<ConflictField, string> = {
  service_name: 'Сервис',
  login: 'Логин',
  account_label: 'Метка',
  urls: 'Адреса',
  vault_id: 'Секция',
  password: 'Пароль',
  notes: 'Заметки',
  totp_secret: 'Ключ TOTP',
}

export function conflictFieldLabel(field: ConflictField): string {
  return LABELS[field]
}

export function isSecretField(field: ConflictField): field is SecretField {
  return field === 'password' || field === 'notes' || field === 'totp_secret'
}

/**
 * Значение метаданного поля версии — как строка для показа.
 *
 * Секретные поля сюда не попадают: их значений в `ConflictVersion` нет по
 * контракту, и подставить их неоткуда, пока человек не открыл их явно.
 */
export function conflictMetaValue(version: ConflictVersion, field: ConflictField): string {
  switch (field) {
    case 'service_name':
      return version.service_name
    case 'login':
      return version.login
    case 'account_label':
      return version.account_label ?? '—'
    case 'urls':
      return version.urls.length === 0 ? '—' : version.urls.join(' · ')
    default:
      return '—'
  }
}

/**
 * Заполнено ли секретное поле в этой версии — из флагов метаданных.
 * У пароля флага нет: он обязателен, и пустым не бывает (§4.1).
 */
export function hasSecret(version: ConflictVersion, field: SecretField): boolean {
  if (field === 'notes') return version.has_notes
  if (field === 'totp_secret') return version.has_totp
  return true
}

/** «отличаются: логин · пароль · заметки» — строка подвала из макета. */
export function differingLine(conflict: RecordConflict): string {
  if (conflict.differing_fields.length === 0) {
    return 'версии совпадают по всем полям'
  }
  const names = conflict.differing_fields.map((field) => conflictFieldLabel(field).toLowerCase())
  return `отличаются: ${names.join(' · ')}`
}

/**
 * Какая версия свежее. Только для подписи «эта правка позже»: выбор всё равно
 * за человеком, и подставлять «более свежую» по умолчанию система не должна —
 * позже не значит нужнее (§5.5).
 */
export function laterSide(conflict: RecordConflict): 'local' | 'remote' | null {
  const local = Date.parse(conflict.local.updated_at)
  const remote = Date.parse(conflict.remote.updated_at)
  if (Number.isNaN(local) || Number.isNaN(remote) || local === remote) return null
  return local > remote ? 'local' : 'remote'
}
