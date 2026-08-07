// @vitest-environment node
import { beforeEach, describe, expect, it } from 'vitest'

import type { CoreErrorCode } from '../contract'
import { PAIRING_MANUAL_CODE_LENGTH } from '../contract'
import { isCoreError } from '../errors'
import {
  createMockCoreClient,
  FINGERPRINT_WORD_COUNT,
  MOCK_MASTER_PASSWORD,
  MOCK_VAULT_WORK,
  normalizePairingInput,
  PAIRING_CODE_TTL_MS,
  PAIRING_PAYLOAD_PREFIX,
  type MockCoreClient,
} from '../mock'

/**
 * Сопряжение устройств в фейк-ядре (F8, §2.2).
 *
 * Проверяем обещания, на которые опирается экран: код одноразовый и живёт
 * ограниченное время, слова-отпечаток появляются ДО того, как устройство стало
 * доверенным, а записи локальных секций на новое устройство не уезжают.
 */

async function expectCoreError(promise: Promise<unknown>, code: CoreErrorCode): Promise<void> {
  let thrown: unknown = null
  let resolved = false

  try {
    await promise
    resolved = true
  } catch (error) {
    thrown = error
  }

  expect(resolved, `ожидалась ошибка ядра ${code}, но промис зарезолвился`).toBe(false)
  expect(isCoreError(thrown, code), `ожидалась ошибка ядра ${code}, получено: ${thrown}`).toBe(true)
}

/** Заведомо чужой код: свой мок отвергает, и правильно делает. */
const FOREIGN_CODE = '4TQ9MB'

let core: MockCoreClient

beforeEach(async () => {
  core = createMockCoreClient({ latencyMs: 0 })
  await core.unlock(MOCK_MASTER_PASSWORD)
})

describe('код для второго устройства', () => {
  it('отдаёт матрицу QR и тот же код для ручного ввода', async () => {
    const offer = await core.getPairingPayload()

    expect(offer.qr.rows).toHaveLength(offer.qr.size)
    expect(offer.qr.rows.every((row) => row.length === offer.qr.size)).toBe(true)
    expect(offer.qr.rows.join('')).toMatch(/^[01]+$/)
    expect(offer.manual_code).toHaveLength(PAIRING_MANUAL_CODE_LENGTH)
    // Ни `0`/`O`, ни `1`/`I`: код набирают руками и диктуют вслух.
    expect(offer.manual_code).toMatch(/^[2-9A-HJ-NP-Z]+$/)
  })

  it('рисует поисковые узоры по углам — иначе это не похоже на код', async () => {
    const { qr } = await core.getPairingPayload()
    const dark = (x: number, y: number) => qr.rows[y]?.[x] === '1'

    // Тёмный центр 3×3 в каждом из трёх углов.
    expect(dark(3, 3)).toBe(true)
    expect(dark(qr.size - 4, 3)).toBe(true)
    expect(dark(3, qr.size - 4)).toBe(true)
    // Светлое кольцо вокруг центра.
    expect(dark(3, 5)).toBe(false)
  })

  it('ЗАКОН №1: сам пейлоад через границу не проходит — только его картинка', async () => {
    const offer = await core.getPairingPayload()

    // В ответе ровно четыре поля: одноразовому ключу сеанса незачем
    // существовать во фронте ещё и строкой.
    expect(Object.keys(offer).sort()).toEqual(['expires_at', 'manual_code', 'qr', 'session_id'])
    expect(JSON.stringify(offer)).not.toContain(PAIRING_PAYLOAD_PREFIX)
  })

  it('каждый запрос даёт новый код: одноразовый на то и одноразовый', async () => {
    const first = await core.getPairingPayload()
    const second = await core.getPairingPayload()

    expect(second.manual_code).not.toBe(first.manual_code)
    expect(second.session_id).not.toBe(first.session_id)
    expect(second.qr.rows).not.toEqual(first.qr.rows)
  })

  it('код живёт ограниченное время', async () => {
    const at = new Date('2026-08-08T10:00:00.000Z')
    const fixed = createMockCoreClient({ latencyMs: 0, now: () => at })
    await fixed.unlock(MOCK_MASTER_PASSWORD)

    const offer = await fixed.getPairingPayload()

    expect(Date.parse(offer.expires_at) - at.getTime()).toBe(PAIRING_CODE_TTL_MS)
  })

  it('на закрытом хранилище кода нет: он даёт право забрать копию', async () => {
    const locked = createMockCoreClient({ latencyMs: 0 })

    await expectCoreError(locked.getPairingPayload(), 'LOCKED')
    await expectCoreError(locked.submitPairedKey(FOREIGN_CODE), 'LOCKED')
    await expectCoreError(locked.confirmPairing('нет такого'), 'LOCKED')
  })

  it('замок стирает выданный код: сопряжение не переживает блокировку', async () => {
    const offer = await core.getPairingPayload()
    core.control.forceLock('timeout')
    await core.unlock(MOCK_MASTER_PASSWORD)

    // Прежний код ядру больше не знаком — он не «свой» и не «истёкший».
    const handshake = await core.submitPairedKey(offer.manual_code)
    expect(handshake.session_id).toBeTruthy()
  })
})

describe('чтение чужого кода', () => {
  it('принимает и полный пейлоад из QR, и код, набранный руками', async () => {
    const fromQr = await core.submitPairedKey(
      `${PAIRING_PAYLOAD_PREFIX}4tq9mb.0123456789abcdef0123456789abcdef`,
    )
    const byHand = await core.submitPairedKey(' 4tq · 9mb ')

    // Код тот же — значит, и слова для сверки те же на обоих устройствах.
    expect(byHand.fingerprint_words).toEqual(fromQr.fingerprint_words)
    expect(byHand.peer_name).toBe(fromQr.peer_name)
  })

  it('даёт ровно четыре слова для сверки глазами (§2.2)', async () => {
    const handshake = await core.submitPairedKey(FOREIGN_CODE)

    expect(handshake.fingerprint_words).toHaveLength(FINGERPRINT_WORD_COUNT)
    expect(handshake.fingerprint_words.every((word) => word.length > 2)).toBe(true)
  })

  it('отказывается от мусора понятным сообщением', async () => {
    await expectCoreError(core.submitPairedKey('привет'), 'VALIDATION')
    // Символы, которых в алфавите нет намеренно: `0`, `O`, `1`, `I`.
    await expectCoreError(core.submitPairedKey('0O1I0O'), 'VALIDATION')
    await expectCoreError(core.submitPairedKey(`${PAIRING_PAYLOAD_PREFIX}коротко`), 'VALIDATION')
  })

  it('не даёт сопрячься с самим собой', async () => {
    const offer = await core.getPairingPayload()

    await expectCoreError(core.submitPairedKey(offer.manual_code), 'VALIDATION')
  })

  it('на свой же устаревший код отвечает «истёк», а не «непонятно»', async () => {
    const stale = await core.getPairingPayload()
    // Новый код выдан — прежний перестал действовать сразу, а не дожил свой срок.
    await core.getPairingPayload()

    await expectCoreError(core.submitPairedKey(stale.manual_code), 'PAIRING_EXPIRED')
  })
})

describe('подтверждение сопряжения', () => {
  it('до подтверждения человеком устройство не доверенное', async () => {
    const handshake = await core.submitPairedKey(FOREIGN_CODE)
    await core.cancelPairing(handshake.session_id)

    // Сеанс закрыт: подтверждать больше нечего.
    await expectCoreError(core.confirmPairing(handshake.session_id), 'NOT_FOUND')
  })

  it('после сверки слов записывает устройство и отчитывается о переносе', async () => {
    const handshake = await core.submitPairedKey(FOREIGN_CODE)
    const result = await core.confirmPairing(handshake.session_id)

    expect(result.device.name).toBe(handshake.peer_name)
    expect(result.device.is_this_device).toBe(false)
    expect(result.device.revoked_at).toBeNull()
    expect(result.duration_ms).toBeGreaterThan(0)
  })

  it('записи локальных секций на новое устройство не уезжают (§4.2)', async () => {
    const handshake = await core.submitPairedKey(FOREIGN_CODE)
    const withLocal = await core.confirmPairing(handshake.session_id)

    // В сиде «Рабочее» локальное: три живые записи синхронизируются, одна нет.
    const live = (await core.listRecords()).length
    expect(withLocal.records_transferred).toBeLessThan(live)

    // Включили синхронизацию секции — её записи начинают ездить.
    await core.setVaultSync(MOCK_VAULT_WORK, true)
    const second = await core.submitPairedKey('9MB4TQ')
    const allSynced = await core.confirmPairing(second.session_id)

    expect(allSynced.records_transferred).toBe(live)
  })

  it('дважды подтвердить один сеанс нельзя', async () => {
    const handshake = await core.submitPairedKey(FOREIGN_CODE)
    await core.confirmPairing(handshake.session_id)

    await expectCoreError(core.confirmPairing(handshake.session_id), 'NOT_FOUND')
  })

  it('просроченный сеанс не подтверждается', async () => {
    let clock = new Date('2026-08-08T10:00:00.000Z')
    const fixed = createMockCoreClient({ latencyMs: 0, now: () => clock })
    await fixed.unlock(MOCK_MASTER_PASSWORD)

    const handshake = await fixed.submitPairedKey(FOREIGN_CODE)
    clock = new Date(clock.getTime() + PAIRING_CODE_TTL_MS + 1000)

    await expectCoreError(fixed.confirmPairing(handshake.session_id), 'PAIRING_EXPIRED')
  })

  it('отмена идемпотентна: по кнопке «Отмена» можно нажать дважды', async () => {
    const handshake = await core.submitPairedKey(FOREIGN_CODE)

    await expect(core.cancelPairing(handshake.session_id)).resolves.toBeUndefined()
    await expect(core.cancelPairing(handshake.session_id)).resolves.toBeUndefined()
  })
})

describe('разбор прочитанного', () => {
  it('приводит к каноническому виду что угодно читаемое', () => {
    expect(normalizePairingInput('4tq9mb')).toBe('4TQ9MB')
    expect(normalizePairingInput('4TQ · 9MB')).toBe('4TQ9MB')
    expect(normalizePairingInput('4TQ-9MB\n')).toBe('4TQ9MB')
    expect(normalizePairingInput(`${PAIRING_PAYLOAD_PREFIX}4tq9mb.${'ab'.repeat(16)}`)).toBe(
      '4TQ9MB',
    )
  })

  it('не выдаёт за код то, что кодом не является', () => {
    expect(normalizePairingInput('')).toBeNull()
    expect(normalizePairingInput('4TQ9M')).toBeNull()
    expect(normalizePairingInput('4TQ9MBB')).toBeNull()
    // Полный пейлоад без ключа — обрезанный QR, а не код.
    expect(normalizePairingInput(`${PAIRING_PAYLOAD_PREFIX}4tq9mb`)).toBeNull()
  })
})
