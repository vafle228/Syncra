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

/// То же для приехавшей версии, ждущей разрешения конфликта (§5.5).
///
/// Своё пространство, а не [`field_aad`], по той же причине, по какой оно есть у
/// секрета устройства: с общим AAD шифротекст из `conflicts` и шифротекст из
/// `records` стали бы взаимозаменяемы, и тот, кто может писать в файл, подставил
/// бы приехавший пароль на место настоящего. Привязка к месту — весь смысл AAD,
/// и «место» здесь включает таблицу.
pub fn conflict_field_aad(record_id: &str, field: &str) -> Vec<u8> {
    format!("syncra:conflict:{record_id}:{field}").into_bytes()
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

// ---------------------------------------------------------------------------
// Пара ключей устройства (§2.1)
// ---------------------------------------------------------------------------

/// Длина секрета устройства: `ed25519_seed(32) || x25519_secret(32)`.
///
/// Двух ключей два независимых сида, а не один, пересчитанный в другой:
/// подпись и обмен ключами — разные задачи, и общий секрет между ними связал бы
/// стойкость одного со стойкостью другого без всякой на то нужды.
pub const DEVICE_SECRET_LEN: usize = 64;
/// Длина публичной половины: `ed25519_pub(32) || x25519_pub(32)`.
///
/// Корень доверия — первая половина (подпись, §2.1); вторая едет вместе с ней,
/// потому что защищённый канал (§3.2) строится на ней же и спрашивать её у
/// устройства повторно, уже по сети, значило бы спрашивать у непроверенного.
pub const DEVICE_PUBLIC_LEN: usize = 64;

/// Своё пространство AAD: секрет устройства нельзя подставить на место пароля
/// записи и наоборот — ровно та же защита, что даёт [`field_aad`] полям.
const DEVICE_SECRET_AAD: &[u8] = b"syncra:device:secret";

const HALF: usize = DEVICE_SECRET_LEN / 2;

/// Долговременная пара ключей устройства (§2.1).
///
/// В файле лежит запечатанной ключом хранилища: украденный файл не даёт права
/// представляться этим устройством, пока не подобран мастер-пароль.
pub struct DeviceKeypair {
    secret: Zeroizing<[u8; DEVICE_SECRET_LEN]>,
}

impl DeviceKeypair {
    /// Новая пара. Случайность — у ОС, как и везде в этом модуле.
    pub fn generate() -> CoreResult<Self> {
        let mut secret: Zeroizing<[u8; DEVICE_SECRET_LEN]> =
            Zeroizing::new([0u8; DEVICE_SECRET_LEN]);
        secret[..HALF].copy_from_slice(&random_bytes::<HALF>()?);
        secret[HALF..].copy_from_slice(&random_bytes::<HALF>()?);
        Ok(Self { secret })
    }

    /// Запечатать для хранения в `meta`.
    pub fn seal(&self, key: &VaultKey) -> CoreResult<Vec<u8>> {
        seal(key, DEVICE_SECRET_AAD, &self.secret[..])
    }

    /// Распечатать из `meta`. Неверный ключ хранилища и испорченный блоб дают
    /// одну и ту же ошибку — как и у остальных секретов.
    pub fn open(key: &VaultKey, blob: &[u8]) -> CoreResult<Self> {
        let plaintext = Zeroizing::new(open(key, DEVICE_SECRET_AAD, blob)?);
        let bytes: [u8; DEVICE_SECRET_LEN] = plaintext[..]
            .try_into()
            .map_err(|_| CoreError::internal("Ключ устройства повреждён."))?;
        Ok(Self {
            secret: Zeroizing::new(bytes),
        })
    }

    /// Публичная половина: то, что уезжает второму устройству и ложится в
    /// `devices`. Считается из секрета каждый раз, а не хранится рядом с ним:
    /// две записи одного и того же расходятся, одна — нет.
    pub fn public_key(&self) -> [u8; DEVICE_PUBLIC_LEN] {
        let mut public = [0u8; DEVICE_PUBLIC_LEN];
        public[..HALF].copy_from_slice(self.signing_key().verifying_key().as_bytes());
        public[HALF..]
            .copy_from_slice(x25519_dalek::PublicKey::from(&self.agreement_secret()).as_bytes());
        public
    }

    /// Ключ подписи (§2.1). Понадобится рукопожатию в S3 и сопряжению в S2.
    pub fn signing_key(&self) -> ed25519_dalek::SigningKey {
        let mut seed = Zeroizing::new([0u8; HALF]);
        seed.copy_from_slice(&self.secret[..HALF]);
        ed25519_dalek::SigningKey::from_bytes(&seed)
    }

    /// Секрет обмена ключами (§3.2). Понадобится ECDH в S3.
    pub fn agreement_secret(&self) -> x25519_dalek::StaticSecret {
        let mut bytes = Zeroizing::new([0u8; HALF]);
        bytes.copy_from_slice(&self.secret[HALF..]);
        x25519_dalek::StaticSecret::from(*bytes)
    }
}

/// Перешифровать запечатанный секрет устройства с одного ключа хранилища на
/// другой. Нужна ровно одному месту — смене мастер-пароля, — и существует
/// отдельно, чтобы распечатанная пара не жила в вызывающем коде дольше строки.
pub fn reseal_device_secret(from: &VaultKey, to: &VaultKey, blob: &[u8]) -> CoreResult<Vec<u8>> {
    DeviceKeypair::open(from, blob)?.seal(to)
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

    #[test]
    fn device_keypair_survives_a_seal_and_open() {
        let key = test_key(7);
        let pair = DeviceKeypair::generate().unwrap();
        let blob = pair.seal(&key).unwrap();

        assert_eq!(
            DeviceKeypair::open(&key, &blob).unwrap().public_key(),
            pair.public_key()
        );
    }

    #[test]
    fn device_secret_does_not_open_with_someone_elses_vault_key() {
        let blob = DeviceKeypair::generate()
            .unwrap()
            .seal(&test_key(7))
            .unwrap();

        assert!(DeviceKeypair::open(&test_key(8), &blob).is_err());
    }

    #[test]
    fn device_secret_is_bound_to_its_own_aad() {
        let key = test_key(7);
        let blob = DeviceKeypair::generate().unwrap().seal(&key).unwrap();

        // Секрет устройства нельзя выдать за секрет записи и наоборот.
        assert!(open(&key, &field_aad("rec-1", "password"), &blob).is_err());
    }

    #[test]
    fn public_key_is_derived_not_remembered() {
        let pair = DeviceKeypair::generate().unwrap();

        // Дважды посчитанная публичная половина одинакова, а у другой пары — другая.
        assert_eq!(pair.public_key(), pair.public_key());
        assert_ne!(
            pair.public_key(),
            DeviceKeypair::generate().unwrap().public_key()
        );
    }

    #[test]
    fn signing_and_agreement_halves_are_independent() {
        let pair = DeviceKeypair::generate().unwrap();
        let public = pair.public_key();

        // Публичная половина — это ровно два разных ключа, а не один дважды.
        assert_ne!(&public[..32], &public[32..]);
    }

    #[test]
    fn resealing_keeps_the_same_device() {
        let (old_key, new_key) = (test_key(7), test_key(8));
        let pair = DeviceKeypair::generate().unwrap();
        let blob = pair.seal(&old_key).unwrap();

        let moved = reseal_device_secret(&old_key, &new_key, &blob).unwrap();

        // Смена мастер-пароля меняет ключ хранилища, но не идентичность устройства.
        assert_eq!(
            DeviceKeypair::open(&new_key, &moved).unwrap().public_key(),
            pair.public_key()
        );
        assert!(DeviceKeypair::open(&old_key, &moved).is_err());
    }
}
