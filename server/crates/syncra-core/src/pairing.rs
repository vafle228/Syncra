//! Сопряжение устройств: формат кода, QR и подпись пейлоада (F8, §2.2).
//!
//! Этот модуль — вся арифметика формата и ни одного обращения к хранилищу:
//! сеанс, замок и запись в `devices` живут в [`crate::session`], доверие — в
//! [`crate::trust`].
//!
//! **Почему пейлоад несёт ключ.** Сопряжение — это перенос публичного ключа по
//! доверенному внеполосному каналу (§2.2): человек смотрит на второй экран, и
//! посередине никого нет. Поэтому в QR лежит не «идентификатор сеанса», а сам
//! ключ устройства: прочитавший код узнаёт корень доверия сразу и целиком, не
//! спрашивая ни у кого по сети.
//!
//! **Что из этого следует для шести символов.** Ключ в шесть символов не
//! помещается, и код для ручного ввода — это не ключ, а имя сеанса: по нему
//! устройство ищут в локальной сети. Поиска пока нет (S3), и `submit_paired_key`
//! отвечает на голый код честным «рядом такого нет» — см. `session.rs`.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use qrcode::{Color, EcLevel, QrCode};
use sha2::{Digest, Sha256};

use crate::crypto::{self, DEVICE_PUBLIC_LEN};
use crate::error::{CoreError, CoreResult};
use crate::model::{DeviceKind, IsoDateTime, QrMatrix};

/// Сколько живёт код (`PAIRING_CODE_TTL_MS`, `mock/pairing.ts:28`). Три минуты:
/// столько нужно, чтобы дойти до второго устройства, и мало, чтобы забытым на
/// экране кодом кто-то воспользовался.
pub const PAIRING_CODE_TTL_MS: i64 = 3 * 60 * 1000;

/// `PAIRING_MANUAL_CODE_LENGTH` (`contract.ts:699`).
pub const MANUAL_CODE_LENGTH: usize = 6;

/// Алфавит кода для ручного ввода: ни `0`/`O`, ни `1`/`I`/`L`. Код диктуют
/// вслух — похожие начертания здесь стоят человеку лишней попытки.
///
/// Ровно 32 символа, то есть степень двойки: поэтому `байт % 32` равновероятен
/// и отбраковка хвоста (как в `generator::random_index`) здесь не нужна.
const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

/// Префикс полного пейлоада — по нему ядро отличает QR от набранного кода.
/// Разбор регистронезависим; наружу уходит [`PAYLOAD_PREFIX_QR`].
pub const PAYLOAD_PREFIX: &str = "syncra-pair:";

/// Тот же префикс верхним регистром.
///
/// В алфавите QR (`alphanumeric mode`) есть `A–Z`, `0–9`, `-`, `.` и `:`, но нет
/// строчных букв: одна строчная буква переводит весь код в побайтовый режим и
/// заметно увеличивает картинку. Поэтому пейлоад пишется прописными целиком.
const PAYLOAD_PREFIX_QR: &str = "SYNCRA-PAIR:";

/// Версия формата пейлоада. Обязана быть в самом коде: его читает ядро другого
/// устройства — возможно, более старое, чем то, что код нарисовало.
const PAYLOAD_VERSION: u8 = 1;

/// Своё пространство подписи: подпись пейлоада нельзя выдать за подпись
/// рукопожатия (S3) и наоборот.
const SIGNATURE_DOMAIN: &[u8] = b"syncra:pair:v1";

/// Длина одноразового публичного ключа сеанса (X25519).
pub const SESSION_PUBLIC_LEN: usize = 32;

const SIGNATURE_LEN: usize = 64;

/// Потолок длины имени в пейлоаде. Имя приходит из ОС и бывает любым, а код с
/// ним должен остаться читаемым камерой.
const NAME_MAX_BYTES: usize = 190;

const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

// ---------------------------------------------------------------------------
// Полезная часть кода
// ---------------------------------------------------------------------------

/// Что устройство рассказывает о себе тому, кто прочитал его код.
///
/// Ни секретов, ни сетевых адресов: адрес меняется и подделывается, а ключ —
/// нет (§2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingBody {
    pub device_id: String,
    pub name: String,
    pub kind: DeviceKind,
    /// Корень доверия: `ed25519_pub(32) || x25519_pub(32)`.
    pub public_key: [u8; DEVICE_PUBLIC_LEN],
    /// Одноразовый ключ сеанса. В этом шаге им никто не пользуется — защищённый
    /// канал строится в S3, — но место под него занято сразу: менять формат
    /// после того, как ключи разъехались по устройствам, будет нечем.
    pub session_public: [u8; SESSION_PUBLIC_LEN],
}

/// Что прочитали со второго устройства.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// Только шесть символов: ключа в них нет и быть не может.
    Code(String),
    /// Полный пейлоад, разобранный и проверенный по подписи.
    Payload { code: String, body: PairingBody },
}

impl Parsed {
    /// Код сеанса — он есть у обеих форм, и по нему узнаётся свой же код.
    pub fn code(&self) -> &str {
        match self {
            Self::Code(code) => code,
            Self::Payload { code, .. } => code,
        }
    }
}

/// Единая формулировка отказа: это вообще не код Syncra (`mock/pairing.ts:276`).
pub fn unreadable_code() -> CoreError {
    CoreError::validation(
        "Это не похоже на код Syncra. Проверьте шесть символов со второго устройства.",
    )
}

/// Испорченный, но узнаваемый код. Отдельный текст от [`unreadable_code`]:
/// «наберите шесть символов» тут не совет — такой код читали камерой или из файла.
fn damaged_code() -> CoreError {
    CoreError::validation("Код со второго устройства повреждён. Покажите его заново.")
}

/// Код для ручного ввода: `4TQ9MB` в каноническом виде, без разделителей.
pub fn manual_code() -> CoreResult<String> {
    let bytes = crypto::random_bytes::<MANUAL_CODE_LENGTH>()?;
    Ok(bytes
        .iter()
        .map(|byte| CODE_ALPHABET[usize::from(*byte) % CODE_ALPHABET.len()] as char)
        .collect())
}

/// Одноразовый секрет сеанса. Публичная половина уезжает в пейлоаде, приватная
/// остаётся в памяти ядра и умирает вместе с сеансом.
pub fn session_secret() -> CoreResult<x25519_dalek::StaticSecret> {
    let bytes = crypto::random_bytes::<SESSION_PUBLIC_LEN>()?;
    Ok(x25519_dalek::StaticSecret::from(bytes))
}

/// Публичная половина секрета сеанса — та, что уезжает в пейлоаде.
pub fn session_public(secret: &x25519_dalek::StaticSecret) -> [u8; SESSION_PUBLIC_LEN] {
    x25519_dalek::PublicKey::from(secret).to_bytes()
}

/// Срок жизни кода, отсчитанный от переданного «сейчас».
pub fn expires_at(now: DateTime<Utc>) -> IsoDateTime {
    (now + Duration::milliseconds(PAIRING_CODE_TTL_MS)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Вышел ли срок. Время приходит аргументом, а не берётся изнутри: иначе это
/// нельзя проверить тестом, не проспав в нём три минуты (как `autolock_due`).
///
/// Нечитаемый срок считается вышедшим: отказать в сопряжении безопаснее, чем
/// продлить сеанс навсегда.
pub fn is_expired(expires_at: &str, now: DateTime<Utc>) -> bool {
    match DateTime::parse_from_rfc3339(expires_at) {
        Ok(deadline) => deadline.with_timezone(&Utc) <= now,
        Err(_) => true,
    }
}

/// Домен обязательства по коду сеанса (S3).
const CODE_COMMITMENT_DOMAIN: &[u8] = b"syncra:paircode:v1";

/// Длина обязательства. Полный хеш не нужен: он не хранится и не переживает
/// соединения, а короткий кадр меньше говорит наблюдателю о своей природе.
pub const CODE_COMMITMENT_LEN: usize = 16;

/// Обязательство по коду сеанса: то, что уезжает в сеть ВМЕСТО кода.
///
/// Сам код в канал не отдаётся никогда. Канал сопряжения зашифрован, но
/// собеседник на том конце ещё не подтверждён (иначе сопрягаться было бы не с
/// кем), и отдать ему шесть символов значило бы отдать их тому, кто, возможно,
/// как раз их и подбирает.
///
/// В хеш входит транскрипт рукопожатия, поэтому обязательство привязано к
/// конкретному соединению: подслушанное, оно не пригодится в следующем.
/// Проверяющая сторона не «расшифровывает» его, а пересчитывает по своему
/// активному коду и сравнивает.
pub fn code_commitment(transcript: &[u8], code: &str) -> [u8; CODE_COMMITMENT_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(CODE_COMMITMENT_DOMAIN);
    hasher.update((transcript.len() as u64).to_be_bytes());
    hasher.update(transcript);
    hasher.update(code.as_bytes());

    let mut out = [0u8; CODE_COMMITMENT_LEN];
    out.copy_from_slice(&hasher.finalize()[..CODE_COMMITMENT_LEN]);
    out
}

// ---------------------------------------------------------------------------
// Пейлоад
// ---------------------------------------------------------------------------

/// Собрать и подписать пейлоад.
///
/// Подпись — не украшение: она доказывает, что код нарисовал держатель
/// приватного ключа и что его не переписали по дороге. Проверяется публичным
/// ключом **из самого пейлоада** — этого довольно, потому что QR и есть тот
/// доверенный канал, ради которого всё затевалось (§2.2).
pub fn encode(body: &PairingBody, code: &str, signing_key: &SigningKey) -> CoreResult<String> {
    let mut bytes = Vec::with_capacity(256);
    bytes.push(PAYLOAD_VERSION);
    bytes.push(kind_byte(body.kind));
    push_short_string(&mut bytes, &body.device_id)?;
    bytes.extend_from_slice(&body.public_key);
    bytes.extend_from_slice(&body.session_public);
    push_short_string(&mut bytes, &body.name)?;

    let signature = signing_key.sign(&signed_bytes(code, &bytes));
    bytes.extend_from_slice(&signature.to_bytes());

    Ok(format!(
        "{PAYLOAD_PREFIX_QR}{code}.{}",
        base32_encode(&bytes)
    ))
}

/// Разобрать прочитанное: полный пейлоад из QR или код, набранный руками.
///
/// UI не знает формата и не отличает одно от другого — разбирает ядро
/// (`contract.ts:704`).
pub fn parse(raw: &str) -> CoreResult<Parsed> {
    let compact: String = raw.chars().filter(|char| !char.is_whitespace()).collect();

    let looks_like_payload = compact
        .get(..PAYLOAD_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(PAYLOAD_PREFIX));
    if !looks_like_payload {
        return Ok(Parsed::Code(
            normalize_code(&compact).ok_or_else(unreadable_code)?,
        ));
    }

    let (code_raw, encoded) = compact[PAYLOAD_PREFIX.len()..]
        .split_once('.')
        .ok_or_else(unreadable_code)?;
    let code = normalize_code(code_raw).ok_or_else(unreadable_code)?;
    let bytes = base32_decode(encoded).ok_or_else(unreadable_code)?;

    Ok(Parsed::Payload {
        body: decode_body(&code, &bytes)?,
        code,
    })
}

fn decode_body(code: &str, bytes: &[u8]) -> CoreResult<PairingBody> {
    if bytes.len() <= SIGNATURE_LEN {
        return Err(damaged_code());
    }
    let (payload, signature) = bytes.split_at(bytes.len() - SIGNATURE_LEN);

    if payload[0] != PAYLOAD_VERSION {
        return Err(CoreError::validation(
            "Этот код сделан более новой версией Syncra.",
        ));
    }

    let mut cursor = Cursor {
        bytes: payload,
        at: 1,
    };
    let kind = kind_from_byte(cursor.byte()?);
    let device_id = cursor.short_string()?;
    let public_key = cursor.array::<DEVICE_PUBLIC_LEN>()?;
    let session_public = cursor.array::<SESSION_PUBLIC_LEN>()?;
    let name = cursor.short_string()?;
    if !cursor.is_at_end() {
        return Err(damaged_code());
    }

    verify_signature(code, payload, signature, &public_key)?;

    // Проверяется ПОСЛЕ подписи: пустое имя в подписанном коде — это ошибка
    // того, кто его нарисовал, а не подделка, и путать их незачем.
    if device_id.trim().is_empty() || name.trim().is_empty() {
        return Err(damaged_code());
    }

    Ok(PairingBody {
        device_id,
        name,
        kind,
        public_key,
        session_public,
    })
}

fn verify_signature(
    code: &str,
    payload: &[u8],
    signature: &[u8],
    public_key: &[u8; DEVICE_PUBLIC_LEN],
) -> CoreResult<()> {
    let verifying: [u8; 32] = public_key[..32].try_into().map_err(|_| damaged_code())?;
    let verifying = VerifyingKey::from_bytes(&verifying).map_err(|_| damaged_code())?;

    let signature: [u8; SIGNATURE_LEN] = signature.try_into().map_err(|_| damaged_code())?;
    verifying
        .verify(
            &signed_bytes(code, payload),
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| damaged_code())
}

/// Что именно подписывается: домен, код сеанса и тело. Код входит в подпись,
/// поэтому переклеить чужое тело под свой код не выйдет.
fn signed_bytes(code: &str, payload: &[u8]) -> Vec<u8> {
    let mut signed = Vec::with_capacity(SIGNATURE_DOMAIN.len() + code.len() + payload.len());
    signed.extend_from_slice(SIGNATURE_DOMAIN);
    signed.extend_from_slice(code.as_bytes());
    signed.extend_from_slice(payload);
    signed
}

fn kind_byte(kind: DeviceKind) -> u8 {
    match kind {
        DeviceKind::Desktop => 0,
        DeviceKind::Mobile => 1,
    }
}

/// Незнакомый тип — это устройство с платформы, которой ещё нет; показать его
/// десктопной иконкой честнее, чем отказаться сопрягаться (ср. `parse_or_desktop`).
fn kind_from_byte(byte: u8) -> DeviceKind {
    match byte {
        1 => DeviceKind::Mobile,
        _ => DeviceKind::Desktop,
    }
}

pub(crate) fn push_short_string(bytes: &mut Vec<u8>, value: &str) -> CoreResult<()> {
    let value = value.as_bytes();
    if value.len() > NAME_MAX_BYTES {
        return Err(CoreError::internal("Слишком длинное имя устройства."));
    }
    bytes.push(value.len() as u8);
    bytes.extend_from_slice(value);
    Ok(())
}

/// Чтение тела по порядку. Любой выход за край — испорченный код, а не паника.
///
/// Тем же курсором разбирает свои кадры сетевой слой ([`crate::net::wire`]):
/// задача та же — прочитать по порядку недоверенные байты и ни на чём не
/// упасть, — и заводить ради неё второй такой же разбор незачем.
pub(crate) struct Cursor<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) at: usize,
}

impl Cursor<'_> {
    pub(crate) fn byte(&mut self) -> CoreResult<u8> {
        let byte = *self.bytes.get(self.at).ok_or_else(damaged_code)?;
        self.at += 1;
        Ok(byte)
    }

    pub(crate) fn take(&mut self, count: usize) -> CoreResult<&[u8]> {
        let end = self.at.checked_add(count).ok_or_else(damaged_code)?;
        let slice = self.bytes.get(self.at..end).ok_or_else(damaged_code)?;
        self.at = end;
        Ok(slice)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> CoreResult<[u8; N]> {
        self.take(N)?.try_into().map_err(|_| damaged_code())
    }

    pub(crate) fn short_string(&mut self) -> CoreResult<String> {
        let len = usize::from(self.byte()?);
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| damaged_code())
    }

    pub(crate) fn is_at_end(&self) -> bool {
        self.at == self.bytes.len()
    }
}

// ---------------------------------------------------------------------------
// Нормализация кода и base32
// ---------------------------------------------------------------------------

/// Только сам код: разделители выбрасываем, регистр поднимаем
/// (`mock/pairing.ts:91`). Человек, переписывающий код с чужого экрана, не
/// обязан попадать в наш формат.
fn normalize_code(raw: &str) -> Option<String> {
    let code: String = raw
        .chars()
        .filter(|char| !matches!(char, '.' | '·' | '-' | '\u{2013}' | '\u{2014}'))
        .filter(|char| !char.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect();

    if code.chars().count() != MANUAL_CODE_LENGTH {
        return None;
    }
    code.bytes()
        .all(|byte| CODE_ALPHABET.contains(&byte))
        .then_some(code)
}

/// Base32 (RFC 4648) без паддинга.
///
/// Не hex и не base64: алфавит base32 целиком лежит внутри алфавита QR, и код
/// от этого рисуется заметно компактнее, чем побайтовый.
fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut buffer, mut bits) = (0u32, 0u32);

    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32_ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32_ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut buffer, mut bits) = (0u32, 0u32);

    for char in text.chars() {
        if !char.is_ascii() {
            return None;
        }
        let value = BASE32_ALPHABET
            .iter()
            .position(|symbol| *symbol == char.to_ascii_uppercase() as u8)?
            as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// QR
// ---------------------------------------------------------------------------

/// Матрица кода. Наружу уходит она, а не строка пейлоада: одноразовому ключу
/// сеанса незачем существовать ещё и JS-строкой (`contract.ts:636`).
pub fn qr_matrix(payload: &str) -> CoreResult<QrMatrix> {
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M)
        .map_err(|_| CoreError::internal("Не удалось нарисовать код сопряжения."))?;

    let size = code.width();
    let rows = code
        .to_colors()
        .chunks(size)
        .map(|row| {
            row.iter()
                .map(|module| if *module == Color::Dark { '1' } else { '0' })
                .collect()
        })
        .collect();

    Ok(QrMatrix { size, rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::DeviceKeypair;

    fn body(pair: &DeviceKeypair, name: &str) -> PairingBody {
        PairingBody {
            device_id: "e5e1a6b8-0f4e-4f2b-9a1e-7f0d2c3b4a59".to_owned(),
            name: name.to_owned(),
            kind: DeviceKind::Desktop,
            public_key: pair.public_key(),
            session_public: [7u8; SESSION_PUBLIC_LEN],
        }
    }

    #[test]
    fn a_payload_survives_the_round_trip() {
        let pair = DeviceKeypair::generate().unwrap();
        let original = body(&pair, "Стенд");
        let payload = encode(&original, "4TQ9MB", &pair.signing_key()).unwrap();

        assert!(payload.starts_with(PAYLOAD_PREFIX_QR));
        match parse(&payload).unwrap() {
            Parsed::Payload { code, body } => {
                assert_eq!(code, "4TQ9MB");
                assert_eq!(body, original);
            }
            other => panic!("ждали пейлоад, получили {other:?}"),
        }
    }

    #[test]
    fn the_payload_is_written_in_the_alphabet_of_qr() {
        // Одна строчная буква переводит весь код в побайтовый режим и раздувает
        // картинку — в пейлоаде их быть не должно ни одной.
        let pair = DeviceKeypair::generate().unwrap();
        let payload = encode(&body(&pair, "Стенд"), "4TQ9MB", &pair.signing_key()).unwrap();

        assert!(payload.chars().all(|char| char.is_ascii_uppercase()
            || char.is_ascii_digit()
            || ":-.".contains(char)));
        assert_eq!(PAYLOAD_PREFIX_QR.to_lowercase(), PAYLOAD_PREFIX);
    }

    #[test]
    fn a_tampered_payload_does_not_pass_the_signature() {
        let pair = DeviceKeypair::generate().unwrap();
        let payload = encode(&body(&pair, "Стенд"), "4TQ9MB", &pair.signing_key()).unwrap();

        // Одна переставленная буква тела — и код больше не читается.
        let at = payload.len() - 5;
        let swapped = if payload.as_bytes()[at] == b'A' {
            'B'
        } else {
            'A'
        };
        let tampered = format!("{}{swapped}{}", &payload[..at], &payload[at + 1..]);

        assert!(parse(&tampered).is_err());
    }

    #[test]
    fn a_payload_signed_by_someone_else_does_not_pass() {
        let pair = DeviceKeypair::generate().unwrap();
        let impostor = DeviceKeypair::generate().unwrap();

        // Ключ в теле наш, подпись — чужая: ровно то, что делает человек
        // посередине, переклеивая QR.
        let payload = encode(&body(&pair, "Стенд"), "4TQ9MB", &impostor.signing_key()).unwrap();
        assert!(parse(&payload).is_err());
    }

    #[test]
    fn the_code_is_part_of_the_signature() {
        let pair = DeviceKeypair::generate().unwrap();
        let payload = encode(&body(&pair, "Стенд"), "4TQ9MB", &pair.signing_key()).unwrap();
        let (_, tail) = payload.split_once('.').unwrap();

        // Тело то же, код другой — не читается: иначе чужое тело можно было бы
        // подставить под свой код.
        assert!(parse(&format!("{PAYLOAD_PREFIX_QR}9MB4TQ.{tail}")).is_err());
    }

    #[test]
    fn a_hand_typed_code_is_normalized_the_way_the_mock_does_it() {
        for raw in ["4tq9mb", "4TQ · 9MB", "4TQ-9MB\n", " 4tq.9mb "] {
            assert_eq!(
                parse(raw).unwrap(),
                Parsed::Code("4TQ9MB".to_owned()),
                "{raw}"
            );
        }
    }

    #[test]
    fn what_is_not_a_syncra_code_is_refused() {
        for raw in [
            "",
            "привет",
            "0O1I0O",
            "4TQ9M",
            "4TQ9MBB",
            "syncra-pair:коротко",
        ] {
            assert!(parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn manual_codes_are_six_symbols_of_the_alphabet() {
        for _ in 0..64 {
            let code = manual_code().unwrap();
            assert_eq!(code.len(), MANUAL_CODE_LENGTH);
            assert!(
                code.bytes().all(|byte| CODE_ALPHABET.contains(&byte)),
                "{code}"
            );
        }
    }

    #[test]
    fn base32_round_trips_every_tail_length() {
        for len in 0..24usize {
            let bytes: Vec<u8> = (0..len).map(|index| (index * 37 + 11) as u8).collect();
            let text = base32_encode(&bytes);

            assert!(text.bytes().all(|byte| BASE32_ALPHABET.contains(&byte)));
            assert_eq!(base32_decode(&text).unwrap()[..len], bytes[..]);
        }
        assert!(base32_decode("не base32").is_none());
    }

    #[test]
    fn expiry_is_measured_against_the_time_it_is_given() {
        let now = Utc::now();
        let deadline = expires_at(now);

        assert!(!is_expired(&deadline, now));
        assert!(!is_expired(
            &deadline,
            now + Duration::milliseconds(PAIRING_CODE_TTL_MS - 1)
        ));
        assert!(is_expired(
            &deadline,
            now + Duration::milliseconds(PAIRING_CODE_TTL_MS)
        ));
        // Нечитаемый срок считается вышедшим: продлить сеанс навсегда хуже.
        assert!(is_expired("никогда", now));
    }

    #[test]
    fn the_matrix_is_square_and_binary() {
        let pair = DeviceKeypair::generate().unwrap();
        let payload = encode(&body(&pair, "Стенд"), "4TQ9MB", &pair.signing_key()).unwrap();
        let matrix = qr_matrix(&payload).unwrap();

        assert_eq!(matrix.rows.len(), matrix.size);
        assert!(matrix.rows.iter().all(|row| row.len() == matrix.size));
        assert!(matrix
            .rows
            .iter()
            .all(|row| row.bytes().all(|byte| byte == b'0' || byte == b'1')));
        // Поисковый узор левого верхнего угла: тёмный центр 3×3, светлое кольцо.
        assert_eq!(matrix.rows[3].as_bytes()[3], b'1');
        assert_eq!(matrix.rows[3].as_bytes()[5], b'0');
    }
}
