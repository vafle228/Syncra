import { readonly, ref, type Ref } from 'vue'

import type { SecuritySettings } from '@/core/contract'
import { DEFAULT_SECURITY_SETTINGS } from '@/core/contract'

/**
 * Действующие таймауты безопасности — одно место, откуда их читают таймеры
 * буфера обмена и авто-скрытия секрета (F13).
 *
 * Значения приходят из ЯДРА: единственный, кто их сюда пишет, — это
 * `useSecurityStore` после успешной загрузки или сохранения. Никто больше
 * `setSecurityPolicy()` вызывать не должен.
 *
 * Почему это НЕ стор Pinia, хотя выглядит как состояние. `useClipboard` живёт в
 * контекстах без Pinia (и его тесты — тоже), а таймаут это политика приложения,
 * а не состояние экрана: у него нет владельца-компонента, он один на всё
 * приложение и не участвует в devtools-отладке экранов. Стор здесь заставил бы
 * два низкоуровневых composable зависеть от Pinia ради двух чисел.
 *
 * Почему стор ПУШИТ значения, а не composable их тянет: тянуть означало бы
 * вызвать `useSecurityStore()` внутри `useClipboard`, то есть внести Pinia туда,
 * где её может не быть. Инверсия оставляет зависимость в одну сторону.
 *
 * ЗДЕСЬ НЕ ПОЯВИТСЯ ТАЙМЕР АВТОБЛОКИРОВКИ. Бездействие считает ядро и присылает
 * `locked` с `reason: 'timeout'`; `autolock_ms` фронт только показывает. Второй
 * таймер означал бы вторую правду о том, заперто ли хранилище, — и расходиться
 * они начнут в первый же день.
 */

const policy = ref<SecuritySettings>({ ...DEFAULT_SECURITY_SETTINGS })

/**
 * Действующие настройки. Читать в МОМЕНТ действия, а не при создании
 * composable: пользователь может сменить таймаут между открытием экрана и
 * нажатием на «Показать».
 */
export function securityPolicy(): Readonly<Ref<SecuritySettings>> {
  return readonly(policy) as Readonly<Ref<SecuritySettings>>
}

/** Запомнить настройки, полученные от ядра. Единственный писатель — стор. */
export function setSecurityPolicy(next: SecuritySettings): void {
  policy.value = { ...next }
}

/**
 * Вернуть умолчания. Нужно тестам: модульное состояние иначе протекало бы из
 * одного теста в другой.
 */
export function resetSecurityPolicy(): void {
  policy.value = { ...DEFAULT_SECURITY_SETTINGS }
}
