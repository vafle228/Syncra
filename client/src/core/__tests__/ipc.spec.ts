// @vitest-environment node
import { afterEach, describe, expect, it } from 'vitest'

import { COMMAND_NAMES } from '../contract'
import { CoreError, toCoreError } from '../errors'
import {
  createTauriCoreClient,
  initCoreClient,
  resolveCoreMode,
  setCoreClient,
  useCore,
} from '../ipc'

afterEach(() => {
  setCoreClient(null)
})

describe('точка переключения мок ↔ ядро', () => {
  it('явный VITE_CORE перевешивает всё', () => {
    expect(resolveCoreMode({ VITE_CORE: 'tauri', PROD: false })).toBe('tauri')
    expect(resolveCoreMode({ VITE_CORE: 'mock', PROD: true })).toBe('mock')
  })

  it('по умолчанию: прод → ядро, дев и тесты → мок', () => {
    expect(resolveCoreMode({ PROD: true })).toBe('tauri')
    expect(resolveCoreMode({ PROD: false })).toBe('mock')
    expect(resolveCoreMode({})).toBe('mock')
  })

  it('игнорирует мусор в VITE_CORE', () => {
    expect(resolveCoreMode({ VITE_CORE: 'нечто', PROD: true })).toBe('tauri')
  })

  it('initCoreClient("mock") поднимает фейк-ядро и отдаёт его через useCore()', async () => {
    const client = await initCoreClient('mock')

    expect(useCore()).toBe(client)
    await client.unlock('syncra-dev')
    await expect(client.listRecords()).resolves.toEqual(
      expect.arrayContaining([expect.objectContaining({ service_name: expect.any(String) })]),
    )
  })

  it('useCore() без инициализации падает понятной ошибкой', () => {
    expect(() => useCore()).toThrow(/initCoreClient/)
  })
})

describe('имена команд на проводе', () => {
  /*
   * Соответствие «ключ CommandMap → имя на проводе» держит TypeScript
   * (`Record<CommandName, string>`), а вот эти два свойства он не проверяет —
   * и обе поломки молчаливые: дубль имени уводит две команды в один
   * Rust-обработчик, а camelCase на проводе просто не находит обработчика.
   */
  const names = Object.values(COMMAND_NAMES)

  it('уникальны: две команды не уходят в один обработчик', () => {
    expect(new Set(names).size).toBe(names.length)
  })

  it('в snake_case — как договорено с ядром', () => {
    expect(names.filter((name) => !/^[a-z][a-z0-9_]*$/.test(name))).toEqual([])
  })

  it('реальный клиент умеет всё, что объявлено в контракте', () => {
    // Интерфейс проверяет типизация, но забыть строку в объекте она не мешает:
    // метод остался бы `undefined` и упал бы только в проде, в Tauri.
    const client = createTauriCoreClient()
    const missing = Object.keys(COMMAND_NAMES).filter(
      (command) => typeof (client as unknown as Record<string, unknown>)[command] !== 'function',
    )

    expect(missing).toEqual([])
  })
})

describe('нормализация ошибок ядра', () => {
  it('поднимает payload ядра до CoreError', () => {
    const error = toCoreError({ code: 'LOCKED', message: 'Хранилище заблокировано.' })

    expect(error).toBeInstanceOf(CoreError)
    expect(error.code).toBe('LOCKED')
    expect(error.message).toBe('Хранилище заблокировано.')
  })

  it('не протаскивает неизвестную ошибку в сообщение для UI', () => {
    const error = toCoreError({ stack: 'секрет=mock-google-personal-pw', code: 12 })

    expect(error.code).toBe('INTERNAL')
    expect(error.message).not.toContain('mock-google-personal-pw')
  })

  it('пропускает CoreError без изменений', () => {
    const original = new CoreError('NOT_FOUND', 'Запись не найдена.')
    expect(toCoreError(original)).toBe(original)
  })
})
