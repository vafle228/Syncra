import { DEVICE_FORMS, formatAgo } from '@/components/devices/deviceFormat'
import { plural, pluralize } from '@/composables/plural'
import type { SyncStatus } from '@/core/contract'

/**
 * Подписи индикатора синхронизации (F10). Чистые функции: ни команды в ядро,
 * ни секрета, ни состояния — только `SyncStatus` на входе и слова на выходе.
 *
 * Макет («Состояния», §01) требует ОДИН индикатор с шестью значениями и задаёт
 * тон каждому: акцент — подтверждение и живой обмен, жёлтый — «стоит знать, но
 * само разрешится», красный — нужен человек, серый — покой. Отсюда же правило:
 * индикатор ничего не блокирует и не открывает модальных окон.
 *
 * Значений здесь семь, а не шесть: `searching` (discovery, §5.1) макет
 * отдельно не рисовал, свернув его в «идёт обмен». Выглядит он так же —
 * пульсирующая акцентная точка, — но подписан честно: «ищу устройства» и «идёт
 * обмен» это разные ответы на вопрос «чего я жду».
 */

/** Что именно показывает индикатор. */
export type SyncLook = 'ok' | 'searching' | 'syncing' | 'pending' | 'alone' | 'error' | 'conflict'

/** Каким цветом. Ровно четыре тона из макета — больше продукту не нужно. */
export type SyncTone = 'accent' | 'warn' | 'danger' | 'calm'

export interface SyncView {
  look: SyncLook
  tone: SyncTone
  /** Короткая подпись рядом с точкой — в шапке окна. */
  chip: string
  /** Пульсирует ли точка: только там, где прямо сейчас что-то происходит. */
  pulse: boolean
  /** Заголовок панели на экране устройств. */
  title: string
  /** Объяснение под заголовком: что это значит и надо ли что-то делать. */
  body: string
}

const CHANGE_FORMS: [string, string, string] = ['изменение', 'изменения', 'изменений']
const CONFLICT_FORMS: [string, string, string] = ['конфликт', 'конфликта', 'конфликтов']
const WAIT_FORMS: [string, string, string] = ['ждёт', 'ждут', 'ждут']

/** «последний обмен — 2 минуты назад» / «обмена ещё не было». */
export function lastSyncLine(status: SyncStatus, now: Date = new Date()): string {
  if (status.last_sync_at === null) return 'обмена ещё не было'
  return `последний обмен — ${formatAgo(status.last_sync_at, now)}`
}

/**
 * Собрать вид индикатора.
 *
 * Порядок проверок — это приоритет состояний, а не удобство чтения. Конфликт
 * идёт первым: это единственное состояние, которое ждёт человека, и заслонять
 * его сообщением об удачном обмене нельзя. Дальше — то, что происходит прямо
 * сейчас, и лишь в тишине — счётчики.
 *
 * Число конфликтов приходит вторым аргументом, а не полем статуса: список
 * конфликтов держит своё место (`list_conflicts`), и дублировать его счётчиком
 * в статусе значило бы иметь две правды.
 */
export function describeSync(
  status: SyncStatus,
  conflicts: number,
  now: Date = new Date(),
): SyncView {
  if (conflicts > 0) {
    return {
      look: 'conflict',
      tone: 'danger',
      chip: pluralize(conflicts, CONFLICT_FORMS),
      pulse: false,
      title:
        conflicts === 1 ? 'Одну запись изменили в двух местах' : 'Записи изменили в двух местах',
      body: 'Обе версии сохранены целиком — Syncra не выбирает за вас и не склеивает их. Решение ждёт столько, сколько нужно.',
    }
  }

  if (status.phase === 'error') {
    return {
      look: 'error',
      tone: 'warn',
      chip: 'Обмен не удался',
      pulse: false,
      title:
        status.peer_name === null
          ? 'Обмен не удался'
          : `Соединение с «${status.peer_name}» оборвалось`,
      // Жёлтый, а не красный: ничего не потеряно. Красный в Syncra оставлен
      // для настоящей потери данных.
      body: `${status.message ?? 'Последняя порция изменений не доехала.'} Данные целы на обоих устройствах. Следующая попытка — сама, вручную можно раньше.`,
    }
  }

  if (status.phase === 'searching') {
    return {
      look: 'searching',
      tone: 'accent',
      chip: 'Ищу устройства',
      pulse: true,
      title: 'Ищу устройства в этой сети',
      body: 'Syncra синхронизируется только в пределах одной локальной сети: устройства должны быть за одним роутером. Через интернет обмена нет и не будет.',
    }
  }

  if (status.phase === 'syncing') {
    return {
      look: 'syncing',
      tone: 'accent',
      chip: status.peer_name === null ? 'Идёт обмен' : `Обмен с ${status.peer_name}`,
      pulse: true,
      title: 'Устройства обмениваются изменениями',
      body: 'Пульсирующая точка вместо процентов: передаётся только дифф, и обмен занимает секунды. Работать можно, не дожидаясь конца.',
    }
  }

  const pending = status.pending_records.length
  if (pending > 0) {
    return {
      look: 'pending',
      tone: 'warn',
      chip: `${pluralize(pending, CHANGE_FORMS)} ${plural(pending, WAIT_FORMS)}`,
      pulse: false,
      title:
        pending === 1
          ? 'Одно изменение пока живёт только здесь'
          : `${pluralize(pending, CHANGE_FORMS)} пока живут только здесь`,
      // Жёлтый как знание, а не как задача: делать ничего не нужно.
      body: 'Остальные устройства офлайн или вне сети. Изменения сохранены здесь и уедут сами, как только кто-то окажется рядом.',
    }
  }

  if (status.peers_online === 0) {
    return {
      look: 'alone',
      tone: 'calm',
      chip: 'Рядом никого',
      pulse: false,
      title: 'Рядом нет других устройств — и это нормально',
      // Серый покой: это не ошибка, поэтому ни жёлтого, ни кнопки «Повторить».
      body: 'Хранилище работает целиком на этом компьютере. Синхронизация не «сломалась»: ей просто не с кем говорить, пока второе устройство не окажется в той же сети.',
    }
  }

  return {
    look: 'ok',
    tone: 'accent',
    chip: 'Синхронизировано',
    pulse: false,
    title: 'Все устройства согласны друг с другом',
    body: `Рядом ${pluralize(status.peers_online, DEVICE_FORMS)}, ${lastSyncLine(status, now)}. Индикатор подтверждает, а не требует.`,
  }
}
