import type { SecuritySettings, SecuritySettingsPatch } from '../contract'
import {
  AUTOLOCK_OPTIONS_MS,
  CLIPBOARD_CLEAR_OPTIONS_MS,
  DEFAULT_SECURITY_SETTINGS,
  SECRET_REVEAL_OPTIONS_MS,
} from '../contract'
import { CoreError } from '../errors'

/**
 * Настройки безопасности фейк-ядра (F13, «Настройки → Безопасность»).
 *
 * Здесь только проверка и хранение: сами таймауты ИСПОЛНЯЕТ ядро, и в настоящем
 * продукте автоблокировку по бездействию считает Rust. Фейк-ядро таймер
 * бездействия намеренно НЕ заводит — иначе хранилище закрывалось бы само посреди
 * теста. Тесты запирают его через `control.forceLock('timeout')`, то есть по той
 * же дорожке, по которой это делает настоящее ядро.
 */

export function cloneSecuritySettings(settings: SecuritySettings): SecuritySettings {
  return { ...settings }
}

/**
 * Сколько раз можно ошибиться в PIN, прежде чем ядро выключит быстрый вход.
 *
 * Политика ЯДРА; здесь она продублирована, чтобы тесты не сверялись с
 * магическим числом. Мало намеренно: PIN из четырёх цифр перебирается за
 * тысячу попыток, поэтому дешёвым он должен быть только для владельца.
 */
export const MOCK_PIN_ATTEMPTS = 5

/** Ступени, между которыми выбирает пользователь, — по одной на настройку. */
const OPTIONS: Record<keyof SecuritySettings, readonly number[]> = {
  autolock_ms: AUTOLOCK_OPTIONS_MS,
  clipboard_clear_ms: CLIPBOARD_CLEAR_OPTIONS_MS,
  secret_reveal_ms: SECRET_REVEAL_OPTIONS_MS,
}

/** Подпись настройки в сообщении об ошибке — человеку, а не в лог. */
const LABELS: Record<keyof SecuritySettings, string> = {
  autolock_ms: 'Автоблокировка',
  clipboard_clear_ms: 'Очистка буфера',
  secret_reveal_ms: 'Показ секрета',
}

/**
 * Применить патч к настройкам.
 *
 * Значение вне сетки — `VALIDATION`, а не «подберём ближайшее»: если UI прислал
 * то, чего в макете нет, это его ошибка, и молча заменять её на похожее число
 * значит скрыть расхождение вместо того, чтобы его показать.
 *
 * Патч применяется ЦЕЛИКОМ или никак: половина сохранённых настроек хуже, чем
 * ни одной, потому что экран после этого показывал бы неправду.
 */
export function applySecurityPatch(
  current: SecuritySettings,
  patch: SecuritySettingsPatch,
): SecuritySettings {
  const next = cloneSecuritySettings(current)

  for (const key of Object.keys(OPTIONS) as (keyof SecuritySettings)[]) {
    const value = patch[key]
    if (value === undefined) continue

    if (!OPTIONS[key].includes(value)) {
      throw new CoreError(
        'VALIDATION',
        `Настройка «${LABELS[key]}»: значение ${value} мс не из числа доступных.`,
      )
    }
    next[key] = value
  }

  return next
}

/** Настройки, с которыми ядро отдаёт свежесозданное хранилище. */
export function initialSecuritySettings(): SecuritySettings {
  return cloneSecuritySettings(DEFAULT_SECURITY_SETTINGS)
}
