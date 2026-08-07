import type { VaultColor } from '@/core/contract'

/**
 * Цвет метки секции → токен дизайн-системы (F7).
 *
 * На проводе и в ядре живёт ИМЯ ступени палитры, а конкретный цвет знает только
 * тема: в светлой те же метки темнее, иначе их не видно на белом. Поэтому здесь
 * возвращается `var(--sy-vault-…)`, а не готовый `oklch(...)`, — хардкодить
 * цвета в компонентах нельзя.
 */
export function vaultColorVar(color: VaultColor): string {
  return `var(--sy-vault-${color})`
}

/** Названия ступеней для выбора цвета — подписи к кружкам в форме секции. */
export const VAULT_COLOR_NAMES: Record<VaultColor, string> = {
  indigo: 'Индиго',
  amber: 'Янтарь',
  magenta: 'Пурпур',
  mint: 'Мята',
  coral: 'Коралл',
}
