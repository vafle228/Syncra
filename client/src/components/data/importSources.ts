import type { ImportSource } from '@/core/contract'

/**
 * Откуда переносим (F12, §3.10 макета).
 *
 * Инструкции живут во ФРОНТЕ, а не в ядре: это описание чужих программ и их
 * меню, а не свойство формата файла. Ядру нужно знать только имя источника —
 * чтобы понять, как разбирать файл.
 */
export interface ImportSourceInfo {
  key: ImportSource
  /** Две-три буквы для плитки. Иконок из сети мы не тянем (§4.2 CLAUDE.md). */
  tag: string
  name: string
  /** Что это за источник — одной строкой под именем. */
  sub: string
  title: string
  lead: string
  steps: string[]
  /** Как называется файл, который надо принести. */
  file: string
}

export const IMPORT_SOURCE_INFO: ImportSourceInfo[] = [
  {
    key: 'chrome',
    tag: 'CH',
    name: 'Chrome',
    sub: 'встроенный менеджер',
    title: 'Переносим пароли из Chrome',
    lead: 'Chrome отдаёт пароли обычной таблицей — это неприятно, но пока файл не покинул компьютер, всё в порядке.',
    steps: [
      'Откройте chrome://password-manager/settings',
      'Нажмите «Скачать файл» и подтвердите паролем от компьютера',
      'Выберите скачанный файл здесь — мы разберём его и удалим',
    ],
    file: 'файл Chrome Passwords.csv',
  },
  {
    key: 'firefox',
    tag: 'FF',
    name: 'Firefox',
    sub: 'встроенный менеджер',
    title: 'Переносим пароли из Firefox',
    lead: 'Экспорт лежит в «Пароли → ⋯ → Экспортировать логины». Файл тоже незашифрованный, поэтому не задерживайте его на диске.',
    steps: [
      'Откройте about:logins',
      'Меню ⋯ → «Экспортировать логины»',
      'Выберите logins.csv здесь',
    ],
    file: 'файл logins.csv',
  },
  {
    key: '1password',
    tag: '1P',
    name: '1Password',
    sub: 'менеджер паролей',
    title: 'Переносим из 1Password',
    lead: 'Заметки, TOTP-ключи и метки переедут вместе с паролями. Вложения и банковские карты Syncra не хранит — их придётся оставить.',
    steps: [
      'В десктопном приложении: File → Export → All Items',
      'Выберите формат CSV',
      'Выберите файл здесь',
    ],
    file: 'файл 1PasswordExport.csv',
  },
  {
    key: 'bitwarden',
    tag: 'BW',
    name: 'Bitwarden',
    sub: 'менеджер паролей',
    title: 'Переносим из Bitwarden',
    lead: 'JSON-экспорт содержит папки — они станут секциями Syncra. Зашифрованный экспорт тоже читаем: спросим пароль от него.',
    steps: [
      'Настройки → Экспорт хранилища',
      'Формат .json (или .json зашифрованный)',
      'Выберите файл здесь',
    ],
    file: 'файл bitwarden_export.json',
  },
  {
    key: 'keepass',
    tag: 'KP',
    name: 'KeePass',
    sub: 'файл .kdbx',
    title: 'Переносим из KeePass',
    lead: 'Единственный источник, который не требует расшифровки на диске: .kdbx читается как есть — спросим пароль от базы.',
    steps: [
      'Найдите файл базы .kdbx',
      'Выберите его здесь',
      'Введите пароль базы — расшифруем в памяти',
    ],
    file: 'файл базы .kdbx',
  },
  {
    key: 'csv',
    tag: 'CSV',
    name: 'Любой CSV',
    sub: 'ручное сопоставление',
    title: 'Переносим из произвольной таблицы',
    lead: 'Покажем первые строки и попросим указать, где логин, где пароль. Порядок колонок бывает любой.',
    steps: ['Выберите .csv здесь', 'Сопоставьте колонки с полями записи', 'Проверьте предпросмотр'],
    file: 'таблицу .csv',
  },
]

export function importSourceInfo(source: ImportSource): ImportSourceInfo {
  return IMPORT_SOURCE_INFO.find((item) => item.key === source) ?? IMPORT_SOURCE_INFO[0]!
}

/** Подпись статуса строки предпросмотра — словами, как в макете. */
export const IMPORT_STATUS_LABEL = {
  new: 'новая',
  duplicate: 'дубликат',
  no_password: 'без пароля',
} as const

/** `1,8 МБ` — размер файла для человека, а не в байтах. */
export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} Б`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} КБ`
  return `${(bytes / (1024 * 1024)).toFixed(1).replace('.', ',')} МБ`
}
