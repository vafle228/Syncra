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
use crate::model::DeviceKind;
use crate::pairing::{self, Cursor, CODE_COMMITMENT_LEN};

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
