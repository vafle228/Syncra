//! Криптография ядра (§3.1, §8.3 «Crypto»).
//!
//! Своего здесь не изобретается ничего: Argon2id выводит ключ из мастер-пароля,
//! XChaCha20-Poly1305 запечатывает секретные поля. Оба — проверенные крейты из
//! категорий, названных в §8.1 спека.
//!
//! Ключ никогда не покидает процесс ядра и зануляется при `lock()`.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{CoreError, CoreResult};

pub const KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;

/// Ключ шифрования хранилища. `Zeroizing` затирает байты при выходе из области
/// видимости — сам по себе он не защита, но лишней копии ключа в куче не оставляет.
pub type VaultKey = Zeroizing<[u8; KEY_LEN]>;

/// Параметры KDF лежат в самой БД: поменяются умолчания — старое хранилище
/// должно продолжать открываться теми параметрами, которыми его создавали.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Память в КиБ.
    pub m_cost: u32,
    /// Число проходов.
    pub t_cost: u32,
    /// Степень параллелизма.
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        // 64 МиБ / 3 прохода — рекомендация OWASP для Argon2id второго профиля.
        // Заметная, но переносимая на десктопе и на телефоне цена за один unlock.
        Self {
            m_cost: 65_536,
            t_cost: 3,
            p_cost: 1,
        }
    }
}

/// Известный открытый текст, по которому проверяется мастер-пароль.
///
/// В БД лежит он же, но запечатанный ключом. Неудачная распечатка = неверный
/// пароль. Отдельного хеша пароля в хранилище нет намеренно: он был бы вторым,
/// независимым способом проверить догадку по украденному файлу.
const VERIFIER_PLAINTEXT: &[u8] = b"syncra-vault-verifier-v1";
const VERIFIER_AAD: &[u8] = b"syncra:verifier";

/// Случайность берётся у ОС напрямую. Своего генератора у ядра нет и не будет:
/// соль и nonce — единственное, что стоит между двумя одинаковыми паролями и
/// одинаковым шифротекстом.
pub fn random_bytes<const N: usize>() -> CoreResult<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::getrandom(&mut buf)
        .map_err(|_| CoreError::internal("Системе не хватило случайности."))?;
    Ok(buf)
}

pub fn random_salt() -> CoreResult<[u8; SALT_LEN]> {
    random_bytes::<SALT_LEN>()
}

/// Вывод ключа хранилища из мастер-пароля.
pub fn derive_key(master_password: &str, salt: &[u8], params: &KdfParams) -> CoreResult<VaultKey> {
    let argon_params = Params::new(params.m_cost, params.t_cost, params.p_cost, Some(KEY_LEN))
        .map_err(|_| CoreError::internal("Неверные параметры вывода ключа."))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);

    let mut key: VaultKey = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(master_password.as_bytes(), salt, &mut key[..])
        .map_err(|_| CoreError::internal("Не удалось вывести ключ хранилища."))?;
    Ok(key)
}

fn cipher(key: &VaultKey) -> CoreResult<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(&key[..])
        .map_err(|_| CoreError::internal("Ключ хранилища повреждён."))
}

/// Запечатать значение. Формат BLOB: `nonce (24) || ciphertext || tag (16)`.
///
/// `aad` привязывает шифротекст к его месту: см. [`field_aad`].
pub fn seal(key: &VaultKey, aad: &[u8], plaintext: &[u8]) -> CoreResult<Vec<u8>> {
    let nonce_bytes = random_bytes::<NONCE_LEN>()?;
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher(key)?
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CoreError::internal("Не удалось зашифровать значение."))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Распечатать значение. Неверный ключ, испорченный шифротекст и подменённый
/// `aad` неотличимы друг от друга и дают одну и ту же ошибку — так и задумано.
pub fn open(key: &VaultKey, aad: &[u8], blob: &[u8]) -> CoreResult<Vec<u8>> {
    if blob.len() <= NONCE_LEN {
        return Err(CoreError::internal("Зашифрованное значение повреждено."));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let nonce = XNonce::from_slice(nonce_bytes);

    cipher(key)?
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CoreError::internal("Не удалось расшифровать значение."))
}

/// Привязка шифротекста к записи и полю.
///
/// Без неё тот, кто может писать в файл БД, мог бы переставить чужой пароль в
/// свою запись или подсунуть заметку на место пароля: сам по себе шифротекст
/// ничего не знает о том, где он лежит. С AAD такая перестановка перестаёт
/// открываться — AEAD проверяет её как часть подписи.
pub fn field_aad(record_id: &str, field: &str) -> Vec<u8> {
    format!("syncra:record:{record_id}:{field}").into_bytes()
}

/// Запечатать проверочное значение — вызывается один раз при создании хранилища.
pub fn seal_verifier(key: &VaultKey) -> CoreResult<Vec<u8>> {
    seal(key, VERIFIER_AAD, VERIFIER_PLAINTEXT)
}

/// Тот ли это мастер-пароль. `false` вместо ошибки: неугаданный пароль — это
/// ожидаемый исход, а не сбой ядра.
pub fn verify(key: &VaultKey, verifier_blob: &[u8]) -> bool {
    match open(key, VERIFIER_AAD, verifier_blob) {
        Ok(plaintext) => plaintext == VERIFIER_PLAINTEXT,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(byte: u8) -> VaultKey {
        Zeroizing::new([byte; KEY_LEN])
    }

    #[test]
    fn seal_open_round_trip() {
        let key = test_key(7);
        let aad = field_aad("rec-1", "password");
        let blob = seal(&key, &aad, b"correct horse").unwrap();

        assert_eq!(open(&key, &aad, &blob).unwrap(), b"correct horse");
    }

    #[test]
    fn ciphertext_does_not_contain_plaintext() {
        let key = test_key(7);
        let aad = field_aad("rec-1", "password");
        let blob = seal(&key, &aad, b"correct horse").unwrap();

        assert!(!blob.windows(13).any(|window| window == b"correct horse"));
    }

    #[test]
    fn aad_binds_ciphertext_to_its_field() {
        let key = test_key(7);
        let blob = seal(&key, &field_aad("rec-1", "password"), b"secret").unwrap();

        // Тот же ключ, то же значение, другое поле — не открывается.
        assert!(open(&key, &field_aad("rec-1", "notes"), &blob).is_err());
        // ...и другая запись тоже.
        assert!(open(&key, &field_aad("rec-2", "password"), &blob).is_err());
    }

    #[test]
    fn wrong_key_does_not_open() {
        let aad = field_aad("rec-1", "password");
        let blob = seal(&test_key(7), &aad, b"secret").unwrap();

        assert!(open(&test_key(8), &aad, &blob).is_err());
    }

    #[test]
    fn nonce_is_fresh_for_every_seal() {
        let key = test_key(7);
        let aad = field_aad("rec-1", "password");

        // Одно и то же значение дважды даёт разные BLOB — иначе по файлу было бы
        // видно, что у двух записей одинаковый пароль.
        assert_ne!(
            seal(&key, &aad, b"same").unwrap(),
            seal(&key, &aad, b"same").unwrap()
        );
    }

    #[test]
    fn verifier_accepts_only_its_own_key() {
        let params = KdfParams::default();
        let salt = random_salt().unwrap();

        let key = derive_key("правильный-пароль", &salt, &params).unwrap();
        let verifier = seal_verifier(&key).unwrap();

        assert!(verify(&key, &verifier));

        let other = derive_key("неправильный-пароль", &salt, &params).unwrap();
        assert!(!verify(&other, &verifier));
    }

    #[test]
    fn derive_key_is_deterministic_for_same_inputs() {
        let params = KdfParams::default();
        let salt = random_salt().unwrap();

        let first = derive_key("пароль", &salt, &params).unwrap();
        let second = derive_key("пароль", &salt, &params).unwrap();
        assert_eq!(&first[..], &second[..]);

        // Другая соль — другой ключ, даже при том же пароле.
        let elsewhere = derive_key("пароль", &random_salt().unwrap(), &params).unwrap();
        assert_ne!(&first[..], &elsewhere[..]);
    }
}
