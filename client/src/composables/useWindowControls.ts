import { ref } from 'vue'

/**
 * Кнопки окна ОС: свернуть, развернуть, закрыть (F13).
 *
 * ЕДИНСТВЕННОЕ место во фронте, которое говорит с окном операционной системы.
 *
 * Почему это НЕ в `core/ipc.ts`, хотя тоже Tauri: там граница с ЯДРОМ —
 * типизированный канал к крипте, сети и хранилищу, и всё, что через него идёт,
 * подчинено Закону №1. Здесь ничего подобного нет: «свернуть окно» не касается
 * ни секретов, ни данных, и смешивать это с контрактом ядра значило бы размыть
 * то, ради чего контракт существует.
 *
 * В браузере (дев, тесты) окна Tauri нет — методы молча ничего не делают, и это
 * осознанно: видимо задизейбленная кнопка закрытия читается как «приложение
 * сломано», а рамка макета должна выглядеть так же, как в собранном приложении.
 * Настоящее поведение включится, когда Tauri-сторона поднимет окно без
 * системной рамки (`decorations: false`); та настройка живёт в другой папке.
 */

/**
 * Доступны ли настоящие кнопки. `false` в браузере — но кнопки всё равно
 * рисуются: см. выше.
 */
export const windowControlsAvailable = ref(isTauri())

function isTauri(): boolean {
  // Tauri 2 помечает окно этим полем. Проверка через `globalThis`, чтобы код
  // оставался рабочим и в jsdom, и в node-окружении тестов.
  return typeof globalThis !== 'undefined' && '__TAURI_INTERNALS__' in globalThis
}

/**
 * Достать текущее окно Tauri. Импорт ленивый: в браузерном бандле модуль
 * подтягивать незачем, а в тестах он бы просто не завёлся.
 */
async function currentWindow() {
  if (!isTauri()) return null
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    return getCurrentWindow()
  } catch {
    // Плагин окна не подключён — кнопки просто не работают, приложение живёт.
    return null
  }
}

export function useWindowControls() {
  async function minimize(): Promise<void> {
    await (await currentWindow())?.minimize()
  }

  /** Развернуть или вернуть прежний размер — одна кнопка, как в макете. */
  async function toggleMaximize(): Promise<void> {
    await (await currentWindow())?.toggleMaximize()
  }

  /**
   * Закрыть окно. Хранилище при этом запирается ЯДРОМ, а не здесь: замок — его
   * забота, и вешать её на кнопку рамки значило бы, что закрытие мимо этой
   * кнопки (Alt+F4, выключение) оставляло бы хранилище открытым.
   */
  async function close(): Promise<void> {
    await (await currentWindow())?.close()
  }

  return { available: windowControlsAvailable, minimize, toggleMaximize, close }
}
