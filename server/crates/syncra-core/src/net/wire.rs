//! Кадры и сообщения протокола синхронизации (§5.6).
//!
//! Протокол собственный и поверх TCP — так решено в спеке ради гарантии
//! доставки. Здесь только форма байтов: ни сокетов, ни крипты, ни правил.
//!
//! Разбор идёт тем же курсором, что и пейлоад сопряжения
//! ([`crate::pairing::Cursor`]): задача одна и та же — прочитать по порядку
//! недоверенные байты и ни на чём не упасть.

use std::io::{Read, Write};

use crate::crypto::DEVICE_PUBLIC_LEN;
use crate::error::{CoreError, CoreResult};
use crate::model::{DeviceKind, RecordId, VaultId};
use crate::pairing::{self, Cursor, CODE_COMMITMENT_LEN};
use crate::sync::{ManifestEntry, SyncRecord};

/// Версия протокола. Едет открытым текстом в первом же кадре: разговаривают два
/// разных ядра, и одно из них может быть старше другого.
pub const PROTOCOL_VERSION: u8 = 1;

/// Потолок длины кадра.
///
/// Кадры этого шага — десятки байт; мегабайт взят с запасом на манифест и дифф
/// (S4). Потолок нужен не ради экономии, а потому что длину кадра называет
/// сторона на том конце: без него `u32::MAX` в первых четырёх байтах — это
/// просьба выделить четыре гигабайта, и её надо уметь не выполнить.
pub const MAX_FRAME: usize = 1024 * 1024;

/// Длина половины публичного ключа (подпись или обмен ключами).
pub const HALF_PUBLIC_LEN: usize = DEVICE_PUBLIC_LEN / 2;

/// Длина случайной части приветствия.
pub const HELLO_NONCE_LEN: usize = 32;

/// Длина подписи Ed25519.
pub const SIGNATURE_LEN: usize = 64;

/// Испорченный или чужой кадр. Формулировка одна на все случаи намеренно:
/// собеседнику незачем знать, на каком именно байте мы его не поняли.
pub fn malformed() -> CoreError {
    CoreError::internal("Устройство в сети ответило непонятным образом.")
}

// ---------------------------------------------------------------------------
// Кадрирование
// ---------------------------------------------------------------------------

/// Записать кадр: `u32 BE длина || тело`.
pub fn write_frame(stream: &mut impl Write, body: &[u8]) -> CoreResult<()> {
    if body.len() > MAX_FRAME {
        return Err(malformed());
    }
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

/// Прочитать кадр. Таймаут ставится на самом сокете вызывающим — здесь его нет
/// намеренно: время в этом крейте всегда приходит снаружи, чтобы его можно было
/// проверить тестом (ср. `autolock_due`, `pairing::is_expired`).
pub fn read_frame(stream: &mut impl Read) -> CoreResult<Vec<u8>> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;

    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(malformed());
    }
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    Ok(body)
}

// ---------------------------------------------------------------------------
// Приветствие
// ---------------------------------------------------------------------------

/// Кто начал соединение. Входит в подпись: иначе подпись одной стороны можно
/// было бы предъявить как подпись другой (отражённое рукопожатие).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Initiator,
    Responder,
}

impl Role {
    pub fn as_byte(self) -> u8 {
        match self {
            Self::Initiator => 0,
            Self::Responder => 1,
        }
    }

    /// Кто на том конце. Роли всегда парные.
    pub fn opposite(self) -> Self {
        match self {
            Self::Initiator => Self::Responder,
            Self::Responder => Self::Initiator,
        }
    }
}

/// Зачем пришли.
///
/// Два режима, и разница между ними — не в шифровании, а в том, кого пускают.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Обычный разговор доверенных: ключ обязан лежать в `devices` и не быть
    /// отозванным (§2.3). Отсюда поедет синхронизация (S4).
    Trusted,
    /// Сопряжение. Пускает КОГО УГОДНО — и в этом весь смысл: сопряжение затем
    /// и существует, чтобы впустить того, кого в `devices` ещё нет (§2.2).
    /// Поэтому в этом режиме разрешены только два запроса, оба про сопряжение,
    /// и оба требуют знания кода, показанного на экране.
    Pairing,
}

impl Mode {
    pub fn as_byte(self) -> u8 {
        match self {
            Self::Trusted => 0,
            Self::Pairing => 1,
        }
    }

    pub fn from_byte(byte: u8) -> CoreResult<Self> {
        match byte {
            0 => Ok(Self::Trusted),
            1 => Ok(Self::Pairing),
            _ => Err(malformed()),
        }
    }
}

/// Первый кадр с каждой стороны, открытым текстом.
///
/// Открытым — потому что до обмена этими байтами шифровать нечем. Ни имени, ни
/// идентификатора устройства здесь нет: кто перед нами, выясняется подписью, а
/// не тем, что собеседник о себе сказал.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub role: Role,
    pub mode: Mode,
    /// Корень доверия (§2.1): по нему ищут строку в `devices`.
    pub signing_public: [u8; HALF_PUBLIC_LEN],
    /// Половина ECDH этого соединения.
    pub ecdh_public: [u8; HALF_PUBLIC_LEN],
    /// Случайность стороны. В общий секрет она не входит, но входит в
    /// транскрипт: два соединения подряд с теми же ключами дают разные
    /// транскрипты, а значит и разные обязательства по коду.
    pub nonce: [u8; HELLO_NONCE_LEN],
}

impl Hello {
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(3 + HALF_PUBLIC_LEN * 2 + HELLO_NONCE_LEN);
        bytes.push(PROTOCOL_VERSION);
        bytes.push(self.role.as_byte());
        bytes.push(self.mode.as_byte());
        bytes.extend_from_slice(&self.signing_public);
        bytes.extend_from_slice(&self.ecdh_public);
        bytes.extend_from_slice(&self.nonce);
        bytes
    }

    pub fn decode(bytes: &[u8], expected_role: Role) -> CoreResult<Self> {
        let mut cursor = Cursor { bytes, at: 0 };

        if cursor.byte()? != PROTOCOL_VERSION {
            return Err(CoreError::internal(
                "Устройство рядом работает на другой версии Syncra.",
            ));
        }
        // Роль не читается, а сверяется: инициатор обязан представиться
        // инициатором. Иначе его же приветствие можно вернуть ему обратно.
        if cursor.byte()? != expected_role.as_byte() {
            return Err(malformed());
        }
        let mode = Mode::from_byte(cursor.byte()?)?;
        let signing_public = cursor.array::<HALF_PUBLIC_LEN>()?;
        let ecdh_public = cursor.array::<HALF_PUBLIC_LEN>()?;
        let nonce = cursor.array::<HELLO_NONCE_LEN>()?;
        if !cursor.is_at_end() {
            return Err(malformed());
        }

        Ok(Self {
            role: expected_role,
            mode,
            signing_public,
            ecdh_public,
            nonce,
        })
    }
}

// ---------------------------------------------------------------------------
// Сообщения внутри канала
// ---------------------------------------------------------------------------

const TAG_AUTH: u8 = 0x01;
const TAG_PING: u8 = 0x02;
const TAG_PONG: u8 = 0x03;
const TAG_PAIRING_LOOKUP: u8 = 0x10;
const TAG_PAIRING_OFFER: u8 = 0x11;
const TAG_PAIRING_REFUSED: u8 = 0x12;
const TAG_PAIRING_COMPLETE: u8 = 0x13;
const TAG_PAIRING_ACK: u8 = 0x14;
// Обмен записями (S4, §5.3). Своя десятка тегов: сопряжение и синхронизация —
// разные разговоры, и путать их номера незачем.
const TAG_SYNC_MANIFEST: u8 = 0x20;
const TAG_SYNC_ACK: u8 = 0x21;
const TAG_SYNC_NEED: u8 = 0x22;
const TAG_SYNC_FETCH: u8 = 0x23;
const TAG_SYNC_BATCH: u8 = 0x24;
const TAG_SYNC_DONE: u8 = 0x25;
const TAG_SYNC_BUSY: u8 = 0x26;

/// Всё, что ходит по каналу после приветствия. Уже зашифровано.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// Подпись транскрипта. Первый кадр внутри канала — и первое, что вообще
    /// доказывает, кто на том конце.
    Auth {
        signature: [u8; SIGNATURE_LEN],
    },
    /// Живо ли устройство. В S3 это единственное содержательное сообщение
    /// доверенного режима; манифест и дифф приедут сюда в S4.
    Ping,
    Pong,
    /// «Покажи пейлоад сеанса, обязательство по коду — вот». Самого кода здесь
    /// нет и быть не может: см. [`crate::pairing::code_commitment`].
    PairingLookup {
        commitment: [u8; CODE_COMMITMENT_LEN],
    },
    /// Пейлоад показанного кода — тот же, что нарисован в QR.
    PairingOffer {
        payload: String,
    },
    /// Такого сеанса нет, срок вышел или попытки кончились. Все три случая
    /// отвечают одинаково: подбирающему коды незачем знать, теплее или холоднее.
    PairingRefused,
    /// «Я сверил слова и записал тебя» — от прочитавшей стороны показавшей.
    ///
    /// Подписи здесь нет намеренно: канал уже подтвердил ключ подписи
    /// собеседника ([`Msg::Auth`]), и вторая подпись доказывала бы то же самое.
    /// Публичный ключ устройства складывается из ключа канала и этой половины.
    PairingComplete {
        commitment: [u8; CODE_COMMITMENT_LEN],
        device_id: String,
        name: String,
        kind: DeviceKind,
        agreement_public: [u8; HALF_PUBLIC_LEN],
    },
    PairingAck,
    /// Порция манифеста (§5.3, шаг 1).
    ///
    /// Порция, а не весь манифест целиком: хранилище на тысячу записей в один
    /// кадр не влезает, а длину кадра называет сторона на том конце, и потолок
    /// ей поднимать нельзя. `last` закрывает манифест — по нему собеседник и
    /// начинает считать.
    SyncManifest {
        entries: Vec<ManifestEntry>,
        /// Секции, выключенные из синхронизации на стороне отправителя. Едут в
        /// каждой порции: их единицы, и склеивать их отдельно дороже.
        local_vaults: Vec<VaultId>,
        last: bool,
    },
    /// «Порцию принял, продолжай».
    SyncAck,
    /// Ответ на закрытый манифест: что собеседник хочет получить.
    SyncNeed {
        wanted: Vec<RecordId>,
        /// Влез ли список целиком. Обрезанный не закрывает круг: остаток
        /// приедет следующим, и отмечать по нему «сошлись» нельзя.
        complete: bool,
    },
    /// «Давай следующую порцию диффа».
    SyncFetch,
    /// Порция диффа. Ходит в обе стороны: ответом на [`Msg::SyncFetch`] и
    /// собственной посылкой в конце круга.
    SyncBatch {
        records: Vec<SyncRecord>,
        last: bool,
    },
    /// Круг закрыт: столько записей у собеседника правда легло.
    SyncDone {
        applied: u32,
    },
    /// «Сейчас не могу: со мной уже кто-то меняется».
    ///
    /// Отдельный ответ, а не обрыв связи: оба устройства ходят друг к другу
    /// сами, и встречные круги — обычное дело, а не поломка. Обрыв здесь
    /// зажигал бы у соседа «обмен не удался» на ровном месте, хотя всё в
    /// порядке и следующая попытка пройдёт.
    SyncBusy,
}

/// Счётчик элементов списка. Четыре байта, а не один: записей в манифесте
/// бывают тысячи.
fn push_count(bytes: &mut Vec<u8>, count: usize) {
    bytes.extend_from_slice(&(count as u32).to_be_bytes());
}

fn read_count(cursor: &mut Cursor<'_>) -> CoreResult<usize> {
    Ok(u32::from_be_bytes(cursor.array::<4>()?) as usize)
}

fn push_flag(bytes: &mut Vec<u8>, flag: bool) {
    bytes.push(u8::from(flag));
}

/// Флаг читается строго: всё, кроме нуля и единицы, — это не наш кадр.
fn read_flag(cursor: &mut Cursor<'_>) -> CoreResult<bool> {
    match cursor.byte()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(malformed()),
    }
}

fn push_entry(bytes: &mut Vec<u8>, entry: &ManifestEntry) {
    pairing::push_long_string(bytes, &entry.record_id);
    pairing::push_long_string(bytes, &entry.vault_id);
    bytes.extend_from_slice(&entry.version.to_be_bytes());
    pairing::push_long_string(bytes, &entry.updated_at);
    pairing::push_optional_string(bytes, entry.deleted_at.as_deref());
    push_flag(bytes, entry.shared);
}

fn read_entry(cursor: &mut Cursor<'_>) -> CoreResult<ManifestEntry> {
    Ok(ManifestEntry {
        record_id: cursor.long_string()?,
        vault_id: cursor.long_string()?,
        version: i64::from_be_bytes(cursor.array::<8>()?),
        updated_at: cursor.long_string()?,
        deleted_at: cursor.optional_string()?,
        shared: read_flag(cursor)?,
    })
}

fn push_record(bytes: &mut Vec<u8>, record: &SyncRecord) {
    pairing::push_long_string(bytes, &record.record_id);
    pairing::push_long_string(bytes, &record.vault_id);
    pairing::push_long_string(bytes, &record.vault_name);
    pairing::push_long_string(bytes, &record.vault_color);
    pairing::push_long_string(bytes, &record.service_name);
    push_count(bytes, record.urls.len());
    for url in &record.urls {
        pairing::push_long_string(bytes, url);
    }
    pairing::push_long_string(bytes, &record.login);
    pairing::push_optional_string(bytes, record.account_label.as_deref());
    pairing::push_optional_string(bytes, record.password.as_deref());
    pairing::push_optional_string(bytes, record.notes.as_deref());
    pairing::push_optional_string(bytes, record.totp_secret.as_deref());
    bytes.extend_from_slice(&record.version.to_be_bytes());
    pairing::push_long_string(bytes, &record.created_at);
    pairing::push_long_string(bytes, &record.updated_at);
    pairing::push_long_string(bytes, &record.password_updated_at);
    pairing::push_optional_string(bytes, record.deleted_at.as_deref());
}

fn read_record(cursor: &mut Cursor<'_>) -> CoreResult<SyncRecord> {
    let record_id = cursor.long_string()?;
    let vault_id = cursor.long_string()?;
    let vault_name = cursor.long_string()?;
    let vault_color = cursor.long_string()?;
    let service_name = cursor.long_string()?;

    let count = read_count(cursor)?;
    let mut urls = Vec::new();
    for _ in 0..count {
        // Ёмкость под `count` заранее не резервируется: число называет сторона
        // на том конце, и просьба выделить память под четыре миллиарда доменов
        // — ровно то, что выполнять не надо (ср. потолок кадра выше).
        urls.push(cursor.long_string()?);
    }

    Ok(SyncRecord {
        record_id,
        vault_id,
        vault_name,
        vault_color,
        service_name,
        urls,
        login: cursor.long_string()?,
        account_label: cursor.optional_string()?,
        password: cursor.optional_string()?,
        notes: cursor.optional_string()?,
        totp_secret: cursor.optional_string()?,
        version: i64::from_be_bytes(cursor.array::<8>()?),
        created_at: cursor.long_string()?,
        updated_at: cursor.long_string()?,
        password_updated_at: cursor.long_string()?,
        deleted_at: cursor.optional_string()?,
    })
}

impl Msg {
    pub fn encode(&self) -> CoreResult<Vec<u8>> {
        let mut bytes = Vec::with_capacity(96);
        match self {
            Self::Auth { signature } => {
                bytes.push(TAG_AUTH);
                bytes.extend_from_slice(signature);
            }
            Self::Ping => bytes.push(TAG_PING),
            Self::Pong => bytes.push(TAG_PONG),
            Self::PairingLookup { commitment } => {
                bytes.push(TAG_PAIRING_LOOKUP);
                bytes.extend_from_slice(commitment);
            }
            Self::PairingOffer { payload } => {
                bytes.push(TAG_PAIRING_OFFER);
                bytes.extend_from_slice(payload.as_bytes());
            }
            Self::PairingRefused => bytes.push(TAG_PAIRING_REFUSED),
            Self::PairingComplete {
                commitment,
                device_id,
                name,
                kind,
                agreement_public,
            } => {
                bytes.push(TAG_PAIRING_COMPLETE);
                bytes.extend_from_slice(commitment);
                bytes.extend_from_slice(agreement_public);
                bytes.push(match kind {
                    DeviceKind::Desktop => 0,
                    DeviceKind::Mobile => 1,
                });
                pairing::push_short_string(&mut bytes, device_id)?;
                pairing::push_short_string(&mut bytes, name)?;
            }
            Self::PairingAck => bytes.push(TAG_PAIRING_ACK),
            Self::SyncManifest {
                entries,
                local_vaults,
                last,
            } => {
                bytes.push(TAG_SYNC_MANIFEST);
                push_flag(&mut bytes, *last);
                push_count(&mut bytes, entries.len());
                for entry in entries {
                    push_entry(&mut bytes, entry);
                }
                push_count(&mut bytes, local_vaults.len());
                for vault_id in local_vaults {
                    pairing::push_long_string(&mut bytes, vault_id);
                }
            }
            Self::SyncAck => bytes.push(TAG_SYNC_ACK),
            Self::SyncNeed { wanted, complete } => {
                bytes.push(TAG_SYNC_NEED);
                push_flag(&mut bytes, *complete);
                push_count(&mut bytes, wanted.len());
                for record_id in wanted {
                    pairing::push_long_string(&mut bytes, record_id);
                }
            }
            Self::SyncFetch => bytes.push(TAG_SYNC_FETCH),
            Self::SyncBatch { records, last } => {
                bytes.push(TAG_SYNC_BATCH);
                push_flag(&mut bytes, *last);
                push_count(&mut bytes, records.len());
                for record in records {
                    push_record(&mut bytes, record);
                }
            }
            Self::SyncDone { applied } => {
                bytes.push(TAG_SYNC_DONE);
                bytes.extend_from_slice(&applied.to_be_bytes());
            }
            Self::SyncBusy => bytes.push(TAG_SYNC_BUSY),
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> CoreResult<Self> {
        let mut cursor = Cursor { bytes, at: 0 };
        let msg = match cursor.byte()? {
            TAG_AUTH => Self::Auth {
                signature: cursor.array::<SIGNATURE_LEN>()?,
            },
            TAG_PING => Self::Ping,
            TAG_PONG => Self::Pong,
            TAG_PAIRING_LOOKUP => Self::PairingLookup {
                commitment: cursor.array::<CODE_COMMITMENT_LEN>()?,
            },
            TAG_PAIRING_OFFER => {
                let payload = String::from_utf8(cursor.bytes[cursor.at..].to_vec())
                    .map_err(|_| malformed())?;
                cursor.at = cursor.bytes.len();
                Self::PairingOffer { payload }
            }
            TAG_PAIRING_REFUSED => Self::PairingRefused,
            TAG_PAIRING_COMPLETE => {
                let commitment = cursor.array::<CODE_COMMITMENT_LEN>()?;
                let agreement_public = cursor.array::<HALF_PUBLIC_LEN>()?;
                // Незнакомый тип — устройство с платформы, которой ещё нет:
                // показать его десктопной иконкой честнее, чем оборвать
                // сопряжение (ср. `DeviceKind::parse_or_desktop`).
                let kind = match cursor.byte()? {
                    1 => DeviceKind::Mobile,
                    _ => DeviceKind::Desktop,
                };
                Self::PairingComplete {
                    commitment,
                    device_id: cursor.short_string()?,
                    name: cursor.short_string()?,
                    kind,
                    agreement_public,
                }
            }
            TAG_PAIRING_ACK => Self::PairingAck,
            TAG_SYNC_MANIFEST => {
                let last = read_flag(&mut cursor)?;
                let count = read_count(&mut cursor)?;
                let mut entries = Vec::new();
                for _ in 0..count {
                    entries.push(read_entry(&mut cursor)?);
                }
                let count = read_count(&mut cursor)?;
                let mut local_vaults = Vec::new();
                for _ in 0..count {
                    local_vaults.push(cursor.long_string()?);
                }
                Self::SyncManifest {
                    entries,
                    local_vaults,
                    last,
                }
            }
            TAG_SYNC_ACK => Self::SyncAck,
            TAG_SYNC_NEED => {
                let complete = read_flag(&mut cursor)?;
                let count = read_count(&mut cursor)?;
                let mut wanted = Vec::new();
                for _ in 0..count {
                    wanted.push(cursor.long_string()?);
                }
                Self::SyncNeed { wanted, complete }
            }
            TAG_SYNC_FETCH => Self::SyncFetch,
            TAG_SYNC_BATCH => {
                let last = read_flag(&mut cursor)?;
                let count = read_count(&mut cursor)?;
                let mut records = Vec::new();
                for _ in 0..count {
                    records.push(read_record(&mut cursor)?);
                }
                Self::SyncBatch { records, last }
            }
            TAG_SYNC_DONE => Self::SyncDone {
                applied: u32::from_be_bytes(cursor.array::<4>()?),
            },
            TAG_SYNC_BUSY => Self::SyncBusy,
            _ => return Err(malformed()),
        };

        if !cursor.is_at_end() {
            return Err(malformed());
        }
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(msg: Msg) {
        assert_eq!(Msg::decode(&msg.encode().unwrap()).unwrap(), msg);
    }

    #[test]
    fn every_message_survives_the_round_trip() {
        round_trip(Msg::Auth {
            signature: [7u8; SIGNATURE_LEN],
        });
        round_trip(Msg::Ping);
        round_trip(Msg::Pong);
        round_trip(Msg::PairingLookup {
            commitment: [3u8; CODE_COMMITMENT_LEN],
        });
        round_trip(Msg::PairingOffer {
            payload: "SYNCRA-PAIR:4TQ9MB.ABCD".to_owned(),
        });
        round_trip(Msg::PairingRefused);
        round_trip(Msg::PairingComplete {
            commitment: [9u8; CODE_COMMITMENT_LEN],
            device_id: "e5e1a6b8-0f4e-4f2b-9a1e-7f0d2c3b4a59".to_owned(),
            name: "Телефон".to_owned(),
            kind: DeviceKind::Mobile,
            agreement_public: [4u8; HALF_PUBLIC_LEN],
        });
        round_trip(Msg::PairingAck);

        round_trip(Msg::SyncManifest {
            entries: vec![
                sample_entry(),
                ManifestEntry {
                    shared: false,
                    ..sample_entry()
                },
            ],
            local_vaults: vec!["v-local".to_owned()],
            last: true,
        });
        round_trip(Msg::SyncManifest {
            entries: Vec::new(),
            local_vaults: Vec::new(),
            last: false,
        });
        round_trip(Msg::SyncAck);
        round_trip(Msg::SyncNeed {
            wanted: vec!["r1".to_owned(), "r2".to_owned()],
            complete: false,
        });
        round_trip(Msg::SyncFetch);
        round_trip(Msg::SyncBatch {
            records: vec![sample_record(), tombstone()],
            last: true,
        });
        round_trip(Msg::SyncDone { applied: 17 });
        round_trip(Msg::SyncBusy);
    }

    fn sample_entry() -> ManifestEntry {
        ManifestEntry {
            record_id: "e5e1a6b8-0f4e-4f2b-9a1e-7f0d2c3b4a59".to_owned(),
            vault_id: "3f2a1b0c-1111-2222-3333-444455556666".to_owned(),
            version: 7,
            updated_at: "2026-08-23T10:00:00.000Z".to_owned(),
            deleted_at: None,
            shared: true,
        }
    }

    fn sample_record() -> SyncRecord {
        SyncRecord {
            record_id: "e5e1a6b8-0f4e-4f2b-9a1e-7f0d2c3b4a59".to_owned(),
            vault_id: "3f2a1b0c-1111-2222-3333-444455556666".to_owned(),
            vault_name: "Личное".to_owned(),
            vault_color: "indigo".to_owned(),
            service_name: "GitHub".to_owned(),
            urls: vec!["github.com".to_owned(), "gist.github.com".to_owned()],
            login: "octocat".to_owned(),
            account_label: Some("рабочий".to_owned()),
            password: Some("  пробелы по краям — часть пароля  ".to_owned()),
            // Заметка длиннее двухсот пятидесяти пяти байт: короткой строкой с
            // однобайтовой длиной такое не закодировать, и ради этого случая
            // у синхронизации своя, четырёхбайтовая.
            notes: Some("длинная заметка ".repeat(64)),
            totp_secret: None,
            version: 7,
            created_at: "2026-08-01T09:00:00.000Z".to_owned(),
            updated_at: "2026-08-23T10:00:00.000Z".to_owned(),
            password_updated_at: "2026-08-10T12:00:00.000Z".to_owned(),
            deleted_at: None,
        }
    }

    /// Надгробие: секретов нет, есть дата удаления (§5.4).
    fn tombstone() -> SyncRecord {
        SyncRecord {
            password: None,
            notes: None,
            totp_secret: None,
            deleted_at: Some("2026-08-23T11:00:00.000Z".to_owned()),
            version: 8,
            ..sample_record()
        }
    }

    #[test]
    fn a_truncated_batch_of_records_is_refused_and_does_not_panic() {
        let full = Msg::SyncBatch {
            records: vec![sample_record()],
            last: true,
        }
        .encode()
        .unwrap();

        for cut in 0..full.len() {
            assert!(Msg::decode(&full[..cut]).is_err(), "обрезка на {cut}");
        }
        let mut extra = full.clone();
        extra.push(0);
        assert!(Msg::decode(&extra).is_err());
    }

    #[test]
    fn a_promised_count_that_is_not_delivered_is_refused() {
        // Число элементов называет сторона на том конце. Обещать четыре
        // миллиарда записей и не прислать ни одной — это отказ, а не паника и
        // не попытка выделить память под обещание.
        let mut wire = vec![TAG_SYNC_MANIFEST, 1];
        wire.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(Msg::decode(&wire).is_err());

        let mut wire = vec![TAG_SYNC_BATCH, 1];
        wire.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(Msg::decode(&wire).is_err());
    }

    #[test]
    fn a_flag_byte_outside_zero_and_one_is_refused() {
        let mut wire = vec![TAG_SYNC_FETCH];
        assert!(Msg::decode(&wire).is_ok());

        wire = vec![TAG_SYNC_MANIFEST, 2];
        wire.extend_from_slice(&0u32.to_be_bytes());
        wire.extend_from_slice(&0u32.to_be_bytes());
        assert!(Msg::decode(&wire).is_err());
    }

    #[test]
    fn a_batch_of_records_does_not_print_its_passwords() {
        // `Msg` выводит `Debug` производным, и без ручного `Debug` у записи
        // одна отладочная строчка в чужом коде положила бы пароль в лог.
        let printed = format!(
            "{:?}",
            Msg::SyncBatch {
                records: vec![sample_record()],
                last: true,
            }
        );
        assert!(!printed.contains("часть пароля"), "{printed}");
        assert!(printed.contains("<скрыт>"), "{printed}");
    }

    #[test]
    fn a_truncated_message_is_refused_and_does_not_panic() {
        let full = Msg::PairingComplete {
            commitment: [9u8; CODE_COMMITMENT_LEN],
            device_id: "id".to_owned(),
            name: "имя".to_owned(),
            kind: DeviceKind::Desktop,
            agreement_public: [4u8; HALF_PUBLIC_LEN],
        }
        .encode()
        .unwrap();

        for cut in 0..full.len() {
            assert!(Msg::decode(&full[..cut]).is_err(), "обрезка на {cut}");
        }
        // ...и лишний хвост тоже: кадр разобран не целиком — значит, не понят.
        let mut extra = full.clone();
        extra.push(0);
        assert!(Msg::decode(&extra).is_err());
    }

    #[test]
    fn an_unknown_tag_is_refused() {
        assert!(Msg::decode(&[0xff]).is_err());
        assert!(Msg::decode(&[]).is_err());
    }

    #[test]
    fn a_hello_survives_the_round_trip() {
        let hello = Hello {
            role: Role::Initiator,
            mode: Mode::Pairing,
            signing_public: [1u8; HALF_PUBLIC_LEN],
            ecdh_public: [2u8; HALF_PUBLIC_LEN],
            nonce: [3u8; HELLO_NONCE_LEN],
        };
        assert_eq!(
            Hello::decode(&hello.encode(), Role::Initiator).unwrap(),
            hello
        );
    }

    #[test]
    fn a_hello_from_the_wrong_side_is_refused() {
        let hello = Hello {
            role: Role::Initiator,
            mode: Mode::Trusted,
            signing_public: [1u8; HALF_PUBLIC_LEN],
            ecdh_public: [2u8; HALF_PUBLIC_LEN],
            nonce: [3u8; HELLO_NONCE_LEN],
        };
        // Отражённое приветствие: инициатору возвращают его же кадр.
        assert!(Hello::decode(&hello.encode(), Role::Responder).is_err());
    }

    #[test]
    fn a_frame_longer_than_the_ceiling_is_refused() {
        let mut sink = Vec::new();
        assert!(write_frame(&mut sink, &vec![0u8; MAX_FRAME + 1]).is_err());

        // ...и обещание длины со стороны собеседника тоже: выделять четыре
        // гигабайта по чужой просьбе мы не станем.
        let mut wire = (MAX_FRAME as u32 + 1).to_be_bytes().to_vec();
        wire.extend_from_slice(&[0u8; 8]);
        assert!(read_frame(&mut wire.as_slice()).is_err());
    }

    #[test]
    fn a_frame_survives_the_round_trip() {
        let mut wire = Vec::new();
        write_frame(&mut wire, b"").unwrap();
        write_frame(&mut wire, b"content").unwrap();

        let mut reader = wire.as_slice();
        assert_eq!(read_frame(&mut reader).unwrap(), b"");
        assert_eq!(read_frame(&mut reader).unwrap(), b"content");
    }
}
