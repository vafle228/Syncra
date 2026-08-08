import type { Device, RecordMeta, RecordSecrets, Vault } from '../contract'
import type { MockConflictEntry } from './conflicts'
import { createThisDevice } from './pairing'

/**
 * Одна запись фейк-ядра: метаданные + секреты живут раздельно, как в ядре.
 *
 * `has_notes` / `has_totp` в сиде НЕ указываются: фейк-ядро выводит их из
 * самих секретов при загрузке. Так сид физически не может соврать про то,
 * заполнено поле или нет.
 */
export interface MockSeedEntry {
  meta: Omit<RecordMeta, 'has_notes' | 'has_totp'>
  secrets: RecordSecrets
}

/**
 * Выводит метаданные-флаги из самих секретов. Единственный источник правды о
 * том, заполнены ли `notes` / `totp_secret`: настоящее ядро тоже считает их у
 * себя, а не принимает от UI.
 */
export function secretFlags(secrets: RecordSecrets): Pick<RecordMeta, 'has_notes' | 'has_totp'> {
  return {
    has_notes: (secrets.notes ?? '').trim() !== '',
    has_totp: (secrets.totp_secret ?? '').trim() !== '',
  }
}

/**
 * Мастер-пароль мок-ядра. Только для разработки против фейк-ядра —
 * в реальном ядре пароль пользователя не хранится нигде и никогда.
 */
export const MOCK_MASTER_PASSWORD = 'syncra-dev'

export const MOCK_VAULT_PERSONAL = 'vault-personal'
export const MOCK_VAULT_WORK = 'vault-work'

/**
 * Секции фейк-хранилища (F7, §4.2).
 *
 * «Рабочее» намеренно стоит локальным: без такой секции в сиде невозможно
 * увидеть, как выглядит и чем отличается несинхронизируемая секция.
 */
export function createVaultSeed(): Vault[] {
  return [
    {
      vault_id: MOCK_VAULT_PERSONAL,
      name: 'Личное',
      color: 'indigo',
      sync: true,
      is_default: true,
      created_at: '2024-11-19T20:50:00.000Z',
    },
    {
      vault_id: MOCK_VAULT_WORK,
      name: 'Рабочее',
      color: 'amber',
      sync: false,
      is_default: false,
      created_at: '2025-01-22T06:10:00.000Z',
    },
  ]
}

/**
 * Секция свежесозданного хранилища. Ровно одна и по умолчанию: пользователь,
 * который ещё не заводил секций, не должен выбирать между ними.
 */
export function createInitialVault(createdAt: string): Vault {
  return {
    vault_id: MOCK_VAULT_PERSONAL,
    name: 'Личное',
    color: 'indigo',
    sync: true,
    is_default: true,
    created_at: createdAt,
  }
}

export const MOCK_DEVICE_PHONE = 'device-phone'
export const MOCK_DEVICE_LAPTOP = 'device-laptop'

const DAY_MS = 24 * 60 * 60 * 1000

/**
 * Устройства фейк-хранилища (F9, §2.3).
 *
 * Даты считаются ОТ текущего времени, а не зашиты константами, как у записей.
 * Причина: список устройств почти целиком про «когда виделись в последний раз»,
 * и зашитая дата через месяц разработки превратила бы каждое устройство в
 * давно пропавшее. «Не появлялся 41 день» должно оставаться свойством
 * конкретного устройства, а не побочным эффектом того, что сегодня за день.
 */
export function createDeviceSeed(now: Date): Device[] {
  const at = (days: number, ms = 0): string =>
    new Date(now.getTime() - days * DAY_MS - ms).toISOString()

  return [
    createThisDevice(at(627)),
    {
      device_id: MOCK_DEVICE_PHONE,
      name: 'iPhone 14',
      kind: 'mobile',
      is_this_device: false,
      paired_at: at(627),
      last_seen_at: at(0, 6 * 60 * 1000),
      revoked_at: null,
    },
    {
      // Устройство, которое давно не выходило на связь. Без него не увидеть,
      // как выглядит предупреждение — а это ровно тот случай, ради которого
      // §2.3 и существует: «ноутбук пропал, и я это заметил по списку».
      device_id: MOCK_DEVICE_LAPTOP,
      name: 'ThinkPad X1',
      kind: 'desktop',
      is_this_device: false,
      paired_at: at(1003),
      last_seen_at: at(41),
      revoked_at: null,
    },
  ]
}

/**
 * Запись, на которой показывается конфликт версий (F11). Вынесена в константу,
 * потому что на неё ссылаются и сид записей, и сид конфликтов, и тесты.
 */
export const MOCK_RECORD_GITHUB = '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1003'

/**
 * Конфликт версий в фейк-хранилище (F11, §5.5).
 *
 * Он есть в сиде с самого начала: расхождение рождается при обмене с другим
 * устройством (§5.3), а второго устройства в процессе разработки не бывает —
 * без сида экран разрешения конфликта невозможно было бы ни увидеть, ни
 * проверить руками.
 *
 * Номер версии у приехавшей стороны СОВПАДАЕТ с местным, и это не небрежность:
 * конфликт растёт из общего предка, обе стороны честно досчитали 6 → 7. Ровно
 * поэтому `resolve_conflict` принимает сторону, а не номер версии.
 */
export function createConflictSeed(): MockConflictEntry[] {
  return [
    {
      record_id: MOCK_RECORD_GITHUB,
      raised_at: '2026-03-02T08:13:05.000Z',
      device_name: 'iPhone 14',
      version: 7,
      // Позже местной правки (28.02) — экран показывает, какая версия свежее.
      updated_at: '2026-03-02T08:12:40.000Z',
      meta: {
        vault_id: MOCK_VAULT_PERSONAL,
        service_name: 'GitHub',
        // Адрес добавлен на телефоне: расходятся не только секреты.
        urls: ['github.com', 'gist.github.com'],
        login: 'demo-user',
        account_label: null,
      },
      secrets: {
        password: 'mock-github-pw-phone',
        notes: 'Recovery codes: mock-4444 mock-5555 mock-6666',
        // Ключ TOTP одинаковый — в diff он попасть не должен.
        totp_secret: 'MOCKTOTPSECRET3',
      },
    },
  ]
}

/**
 * Сид-данные для разработки.
 *
 * Все «секреты» ниже — заведомо фальшивые строки с префиксом `mock-`.
 * Никогда не класть сюда настоящие учётные данные: файл лежит в репозитории.
 *
 * Набор специально покрывает §4.4: два аккаунта на `google.com`, различимые
 * по `login` / `account_label`, — это норма продукта, а не коллизия.
 */
export function createSeed(): MockSeedEntry[] {
  return [
    {
      meta: {
        record_id: '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1001',
        vault_id: MOCK_VAULT_PERSONAL,
        service_name: 'Google',
        urls: ['accounts.google.com', 'mail.google.com'],
        login: 'personal.demo@gmail.com',
        account_label: 'Личный',
        version: 4,
        created_at: '2025-03-14T08:21:05.000Z',
        updated_at: '2026-01-09T17:44:12.000Z',
        password_updated_at: '2026-01-09T17:44:12.000Z',
        deleted_at: null,
      },
      secrets: {
        password: 'mock-google-personal-pw',
        notes: 'Контрольный вопрос: кличка первого кота.',
        totp_secret: 'MOCKTOTPSECRET1',
      },
    },
    {
      meta: {
        record_id: '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1002',
        vault_id: MOCK_VAULT_WORK,
        service_name: 'Google',
        urls: ['accounts.google.com', 'admin.google.com'],
        login: 'work.demo@syncra.example',
        account_label: 'Рабочий',
        version: 2,
        created_at: '2025-06-02T11:03:47.000Z',
        updated_at: '2025-12-21T09:15:30.000Z',
        password_updated_at: '2025-06-02T11:03:47.000Z',
        deleted_at: null,
      },
      secrets: {
        password: 'mock-google-work-pw',
        notes: null,
        totp_secret: null,
      },
    },
    {
      meta: {
        record_id: MOCK_RECORD_GITHUB,
        vault_id: MOCK_VAULT_PERSONAL,
        service_name: 'GitHub',
        urls: ['github.com'],
        login: 'demo-user',
        account_label: null,
        version: 7,
        created_at: '2024-11-19T20:55:00.000Z',
        updated_at: '2026-02-28T13:02:19.000Z',
        password_updated_at: '2025-08-30T07:41:55.000Z',
        deleted_at: null,
      },
      secrets: {
        password: 'mock-github-pw',
        notes: 'Recovery codes: mock-1111 mock-2222 mock-3333',
        totp_secret: 'MOCKTOTPSECRET3',
      },
    },
    {
      meta: {
        record_id: '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1004',
        vault_id: MOCK_VAULT_PERSONAL,
        service_name: 'Steam',
        urls: ['store.steampowered.com', 'steamcommunity.com'],
        login: 'demo_player',
        account_label: null,
        version: 1,
        created_at: '2025-09-08T18:30:10.000Z',
        updated_at: '2025-09-08T18:30:10.000Z',
        password_updated_at: '2025-09-08T18:30:10.000Z',
        deleted_at: null,
      },
      secrets: {
        password: 'mock-steam-pw',
        notes: null,
        totp_secret: null,
      },
    },
    {
      meta: {
        record_id: '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1005',
        vault_id: MOCK_VAULT_WORK,
        service_name: 'Jira',
        urls: ['syncra.atlassian.example'],
        login: 'd.demo',
        account_label: 'Старый доступ',
        version: 3,
        created_at: '2025-01-22T06:12:00.000Z',
        updated_at: '2026-03-15T10:00:00.000Z',
        password_updated_at: '2025-01-22T06:12:00.000Z',
        // Tombstone (§5.4): в обычном списке не показывается.
        deleted_at: '2026-03-15T10:00:00.000Z',
      },
      // Секреты tombstone-записи фейк-ядро при загрузке сида отбрасывает:
      // у надгробия нет полезной нагрузки.
      secrets: {
        password: 'mock-jira-pw',
        notes: null,
        totp_secret: null,
      },
    },
  ]
}
