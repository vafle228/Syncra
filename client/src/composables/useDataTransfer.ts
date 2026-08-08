import { onScopeDispose, ref } from 'vue'

import type {
  ExportFile,
  ImportOptions,
  ImportPreview,
  ImportResult,
  ImportSource,
} from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore, type Unsubscribe } from '@/core/ipc'

/**
 * Экспорт, импорт и бэкап (F12, §6.2).
 *
 * Почему composable, а не стор — по той же причине, что и у сопряжения (F8).
 * Ни одно из этих состояний не должно пережить уход с экрана: разобранный файл
 * — это чужие пароли, ждущие в ядре согласия, а путь к CSV — адрес файла, в
 * котором лежат все пароли открытым текстом. Такому не место в Pinia,
 * localStorage и снапшотах состояния; проверяется тестом.
 *
 * Мастер-пароль проходит ТРАНЗИТОМ: аргументом в ядро, и тут же отпускается.
 * Ни здесь, ни в вызывающем компоненте он не сохраняется.
 *
 * Содержимого файлов тут нет вообще — ни на входе, ни на выходе. Файл целиком
 * собирает и пишет ядро, разбор чужого файла тоже его дело (см. `ExportFile`
 * и `ImportPreview` в контракте).
 */

/**
 * Подписка на замок: заперли хранилище — на экране не должно остаться ни
 * разобранного файла, ни следа от только что созданного экспорта.
 */
function watchLock(onLocked: () => void): Unsubscribe | null {
  try {
    return useCore().on('locked', onLocked)
  } catch {
    /* ядро ещё не поднято — подписка не критична для рендера */
    return null
  }
}

/** Вынести данные наружу: зашифрованный бэкап или открытый CSV. */
export function useVaultExport(kind: 'csv' | 'backup') {
  /** След созданного файла: путь, размер, число записей. Содержимого нет. */
  const file = ref<ExportFile | null>(null)
  const busy = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)

  function forget(): void {
    file.value = null
    error.value = null
  }

  /**
   * Сделать файл. `masterPassword` уходит в ядро и здесь не остаётся —
   * ни в состоянии, ни в замыкании после возврата.
   */
  async function run(masterPassword: string): Promise<boolean> {
    if (masterPassword === '') {
      error.value = 'Введите мастер-пароль.'
      return false
    }

    busy.value = true
    error.value = null
    try {
      const core = useCore()
      file.value =
        kind === 'csv'
          ? await core.exportCsv(masterPassword)
          : await core.exportBackup(masterPassword)
      return true
    } catch (cause) {
      file.value = null
      error.value = isCoreError(cause) ? cause.message : 'Не удалось создать файл.'
      return false
    } finally {
      busy.value = false
    }
  }

  /**
   * «Удалить файл сейчас» (§3.10 макета). Удаляет ядро — и только тот файл,
   * который само создало.
   */
  async function remove(): Promise<boolean> {
    const current = file.value
    if (current === null) return false

    busy.value = true
    error.value = null
    try {
      await useCore().deleteExport(current.path)
      file.value = null
      return true
    } catch (cause) {
      error.value = isCoreError(cause) ? cause.message : 'Не удалось удалить файл.'
      return false
    } finally {
      busy.value = false
    }
  }

  const unsubscribe = watchLock(forget)

  onScopeDispose(() => {
    forget()
    unsubscribe?.()
  })

  return { file, busy, error, run, remove, forget }
}

/** Значения по умолчанию для мастера импорта. */
export const DEFAULT_IMPORT_OPTIONS: ImportOptions = {
  // Дубликат — это уже сохранённый пароль пользователя. По умолчанию
  // не трогаем то, что у него уже работает.
  skip_duplicates: true,
  flag_reused: true,
}

/** Занести пароли внутрь: мастер импорта из чужого менеджера (§3.10). */
export function useVaultImport() {
  /**
   * Разобранный файл. Паролей здесь нет по контракту: они остались в ядре и
   * ждут подтверждения — либо будут забыты по `cancel`.
   */
  const preview = ref<ImportPreview | null>(null)
  const result = ref<ImportResult | null>(null)
  const options = ref<ImportOptions>({ ...DEFAULT_IMPORT_OPTIONS })
  const busy = ref(false)
  /** Сообщение ядра для показа пользователю. Секретов не содержит по контракту. */
  const error = ref<string | null>(null)

  function reset(): void {
    preview.value = null
    result.value = null
    error.value = null
  }

  /**
   * Выбрать и разобрать файл. Окно выбора открывает ядро — оно же читает файл:
   * ему всё равно предстоит держать чужие пароли, а UI незачем.
   *
   * `false` без ошибки — человек закрыл окно выбора. Это не отказ ядра, и
   * показывать его красным нельзя.
   */
  async function pick(source: ImportSource): Promise<boolean> {
    busy.value = true
    error.value = null
    result.value = null
    try {
      preview.value = await useCore().beginImport(source)
      return preview.value !== null
    } catch (cause) {
      preview.value = null
      error.value = isCoreError(cause) ? cause.message : 'Не удалось разобрать файл.'
      return false
    } finally {
      busy.value = false
    }
  }

  /** Согласие получено: ядро заводит записи. */
  async function commit(): Promise<boolean> {
    const session = preview.value
    if (session === null) return false

    busy.value = true
    error.value = null
    try {
      result.value = await useCore().commitImport(session.session_id, { ...options.value })
      // Сеанс отработал: разобранных строк в ядре больше нет.
      preview.value = null
      return true
    } catch (cause) {
      error.value = isCoreError(cause) ? cause.message : 'Не удалось перенести записи.'
      return false
    } finally {
      busy.value = false
    }
  }

  /** Передумали: ядро забывает разобранные строки вместе с их паролями. */
  async function cancel(): Promise<void> {
    const session = preview.value
    reset()
    if (session === null) return

    try {
      await useCore().cancelImport(session.session_id)
    } catch {
      /* сеанс всё равно закрыт для UI: держать его на экране незачем */
    }
  }

  const unsubscribe = watchLock(reset)

  onScopeDispose(() => {
    // Разобранный файл не должен пережить экран. Отмену шлём «вдогонку»:
    // ядру пора забыть чужие пароли, даже если экран уже закрыт.
    void cancel()
    unsubscribe?.()
  })

  return { preview, result, options, busy, error, pick, commit, cancel, reset }
}
