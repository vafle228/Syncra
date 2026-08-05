import type { RecordMeta, RecordSecrets } from '../contract'

/** Одна запись фейк-ядра: метаданные + секреты живут раздельно, как в ядре. */
export interface MockSeedEntry {
  meta: RecordMeta
  secrets: RecordSecrets
}

/**
 * Мастер-пароль мок-ядра. Только для разработки против фейк-ядра —
 * в реальном ядре пароль пользователя не хранится нигде и никогда.
 */
export const MOCK_MASTER_PASSWORD = 'syncra-dev'

export const MOCK_VAULT_PERSONAL = 'vault-personal'
export const MOCK_VAULT_WORK = 'vault-work'

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
        record_id: '6f1c2e14-4c1e-4a3f-9b4a-1f9f0c7a1003',
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
