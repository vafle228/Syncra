import type { ImportRowStatus, ImportSource } from '../contract'

/**
 * Экспорт, импорт и бэкап в фейк-ядре (F12, §6.2).
 *
 * Фейк-ядро ничего не пишет на диск и ничего не шифрует — оно ИЗОБРАЖАЕТ то,
 * что видно UI: имя файла, путь, размер, разобранные строки. Настоящие запись
 * файла, CSV-сериализация и шифрование бэкапа живут в Rust; крипты здесь нет и
 * быть не должно (CLAUDE.md, Закон №1).
 *
 * Все «пароли» ниже — заведомо фальшивые строки с префиксом `mock-`.
 */

/**
 * Одна строка разобранного файла.
 *
 * Живёт ТОЛЬКО внутри фейк-ядра, между `beginImport` и `commitImport`: в
 * предпросмотр (`ImportPreview`) попадают сайт, логин и статус, но не пароль.
 * Настоящее ядро держит их так же — у себя.
 */
export interface MockImportEntry {
  site: string
  login: string
  /** Пустая строка — «в файле нет пароля»: такие строки не импортируются. */
  password: string
  notes: string | null
  totp_secret: string | null
}

export interface MockImportFile {
  file_name: string
  entries: MockImportEntry[]
}

/** Имя файла, который отдаёт каждый источник (§3.10 макета). */
const SOURCE_FILES: Record<ImportSource, string> = {
  chrome: 'Chrome Passwords.csv',
  firefox: 'logins.csv',
  '1password': '1PasswordExport.csv',
  bitwarden: 'bitwarden_export.json',
  keepass: 'Passwords.kdbx',
  csv: 'passwords.csv',
}

/**
 * Строки, общие для всех источников.
 *
 * Набор подобран так, чтобы предпросмотр показывал все три статуса сразу:
 *  - `github.com / demo-user` уже есть в сид-хранилище — это дубликат;
 *  - у одной строки пустой пароль — так бывает в любом экспорте браузера;
 *  - два сайта делят один пароль — на этом видно «повторяющиеся пароли».
 */
const COMMON_ENTRIES: MockImportEntry[] = [
  {
    site: 'figma.com',
    login: 'demo@fastmail.example',
    password: 'mock-import-figma',
    notes: null,
    totp_secret: null,
  },
  {
    site: 'gosuslugi.ru',
    login: '+7 921 000 00 14',
    password: 'mock-import-shared',
    notes: null,
    totp_secret: null,
  },
  {
    site: 'github.com',
    login: 'demo-user',
    password: 'mock-import-github',
    notes: null,
    totp_secret: null,
  },
  {
    site: 'старый-форум.рф',
    login: 'demo-annka',
    // Пустой пароль: строка в файле есть, пароля в ней нет.
    password: '',
    notes: null,
    totp_secret: null,
  },
  {
    site: 'aliexpress.com',
    login: 'demo@fastmail.example',
    password: 'mock-import-shared',
    notes: null,
    totp_secret: null,
  },
  {
    site: 'booking.com',
    login: 'demo@fastmail.example',
    password: 'mock-import-booking',
    notes: null,
    totp_secret: null,
  },
  {
    site: 'dzen.ru',
    login: 'demo-reader',
    password: 'mock-import-dzen',
    notes: null,
    totp_secret: null,
  },
  {
    site: 'ozon.ru',
    login: 'demo@fastmail.example',
    password: 'mock-import-ozon',
    notes: null,
    totp_secret: null,
  },
  {
    site: 'hh.ru',
    login: 'demo-cv',
    password: 'mock-import-hh',
    notes: null,
    totp_secret: null,
  },
]

/**
 * Что источник умеет отдать сверх логина и пароля. Не украшение: из этого
 * растут флаги `has_notes` / `has_totp` у импортированных записей, а браузеры
 * заметок и TOTP-ключей не отдают вовсе.
 */
const SOURCE_EXTRAS: Record<ImportSource, MockImportEntry[]> = {
  chrome: [],
  firefox: [],
  '1password': [
    {
      site: 'stripe.com',
      login: 'demo-finance',
      password: 'mock-import-stripe',
      notes: 'Резервные коды: mock-7777 mock-8888',
      totp_secret: 'MOCKIMPORTTOTP1',
    },
  ],
  bitwarden: [
    {
      site: 'notion.so',
      login: 'demo@fastmail.example',
      password: 'mock-import-notion',
      notes: 'Из папки «Работа»',
      totp_secret: null,
    },
  ],
  keepass: [
    {
      site: 'nas.local',
      login: 'admin',
      password: 'mock-import-nas',
      notes: 'Локальный доступ к хранилищу файлов',
      totp_secret: null,
    },
  ],
  csv: [],
}

/** Что «прочитало» фейк-ядро из файла выбранного источника. */
export function createImportFile(source: ImportSource): MockImportFile {
  return {
    file_name: SOURCE_FILES[source],
    entries: [...COMMON_ENTRIES, ...SOURCE_EXTRAS[source]].map((entry) => ({ ...entry })),
  }
}

/**
 * Домен строки в сравнимом виде. Матчинг — по адресу и логину (§4.4), а не по
 * имени сервиса: имя не уникально.
 */
export function importHost(site: string): string {
  const trimmed = site.trim().toLowerCase()
  const withoutScheme = trimmed.replace(/^[a-z][a-z0-9+.-]*:\/\//, '')
  const host = withoutScheme.split('/')[0]?.split('?')[0] ?? ''
  return host.replace(/^www\./, '').replace(/:\d+$/, '')
}

/** Статус строки: новая, такая уже есть, пароля нет. */
export function importRowStatus(
  entry: MockImportEntry,
  knows: (host: string, login: string) => boolean,
): ImportRowStatus {
  if (entry.password.trim() === '') return 'no_password'
  return knows(importHost(entry.site), entry.login.trim().toLowerCase()) ? 'duplicate' : 'new'
}

/**
 * Сколько импортированных паролей повторяется на разных сайтах.
 *
 * Считается по значениям паролей, и считает их ЯДРО у себя: чтобы получить это
 * число во фронте, пришлось бы вынести туда все пароли разом — ровно то, чего
 * не делает ни одна команда контракта.
 */
export function countReusedPasswords(entries: MockImportEntry[]): number {
  const seen = new Map<string, number>()
  for (const entry of entries) {
    if (entry.password === '') continue
    seen.set(entry.password, (seen.get(entry.password) ?? 0) + 1)
  }
  return entries.filter((entry) => (seen.get(entry.password) ?? 0) > 1).length
}

function two(value: number): string {
  return String(value).padStart(2, '0')
}

/** «Импорт 01.08» — секция, в которую ложится разобранный файл (§3.10 макета). */
export function importVaultName(now: Date): string {
  return `Импорт ${two(now.getDate())}.${two(now.getMonth() + 1)}`
}

function isoDay(now: Date): string {
  return `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())}`
}

/**
 * Куда ядро кладёт созданный файл. Имя открытого экспорта начинается с `plain`
 * не для красоты: этот файл потом ищут глазами в папке загрузок, чтобы удалить.
 */
export function exportLocation(
  kind: 'csv' | 'backup',
  now: Date,
): { path: string; file_name: string } {
  const file_name =
    kind === 'csv' ? `syncra-plain-${isoDay(now)}.csv` : `syncra-${isoDay(now)}.syncra`
  const folder = kind === 'csv' ? '~/Downloads' : '~/Backups'
  return { path: `${folder}/${file_name}`, file_name }
}

/**
 * Размер файла. Фейк-ядро его не пишет, поэтому считает оценкой по числу
 * записей — а не выдумывает круглую цифру, которая не двигалась бы от того,
 * что в хранилище стало вдвое больше паролей.
 */
export function exportSize(kind: 'csv' | 'backup', records: number): number {
  return kind === 'csv' ? 74 + records * 96 : 4096 + records * 320
}
