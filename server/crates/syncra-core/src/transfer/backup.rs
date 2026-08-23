//! Зашифрованный бэкап (§6.2, резервный путь восстановления §6.3).
//!
//! # Формат файла
//!
//! Читать его будет ядро, которого ещё нет: бэкап делают затем, чтобы открыть
//! его через год на новом компьютере. Поэтому формат описан здесь подробно, а
//! номер версии лежит в самом файле, а не подразумевается.
//!
//! ```text
//! смещение  размер  что
//! 0         14      magic  b"SYNCRA-BACKUP\0"
//! 14        2       версия формата, u16 little-endian  (сейчас 1)
//! 16        1       идентификатор KDF, u8              (1 = Argon2id)
//! 17        4       m_cost, u32 LE   \
//! 21        4       t_cost, u32 LE    > параметры Argon2id, которыми выведен ключ
//! 25        4       p_cost, u32 LE   /
//! 29        1       длина соли, u8    (сейчас 16)
//! 30        N       соль
//! 30+N      ...     полезная нагрузка: crypto::seal → nonce(24) || ct || tag(16)
//! ```
//!
//! **AAD полезной нагрузки — весь заголовок целиком.** Это не украшение: без
//! него в чужом файле можно было бы понизить параметры KDF и заставить чужое
//! ядро считать ключ по слабым — а получатель не заметил бы ничего, потому что
//! пароль-то подошёл бы.
//!
//! Параметры KDF лежат в файле по той же причине, по какой они лежат в `meta`
//! хранилища: поменяются умолчания — старый файл должен продолжать открываться
//! теми параметрами, которыми его закрывали.
//!
//! # Чего в бэкапе нет
//!
//! - **надгробий** (§5.4): бэкап — снимок, а не история. Секретов у надгробия
//!   нет, воскрешать нечего, а «удалено» без второй стороны ничего не значит;
//! - **пары ключей устройства и таблицы `devices`** (§2.1): §6.3 говорит про
//!   «все устройства потеряны». Перенести идентичность в файл значило бы дать
//!   двум машинам право представляться одним устройством. Восстановленное
//!   хранилище заводит НОВУЮ пару и сопрягается заново.

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::crypto::{self, KdfParams, SALT_LEN};
use crate::error::{CoreError, CoreResult};

const MAGIC: &[u8; 14] = b"SYNCRA-BACKUP\0";
/// Версия формата файла. Растёт, когда меняется раскладка заголовка или смысл
/// полезной нагрузки, — и никогда не молча.
pub const FORMAT_VERSION: u16 = 1;
const KDF_ARGON2ID: u8 = 1;
const HEADER_LEN: usize = MAGIC.len() + 2 + 1 + 4 * 3 + 1 + SALT_LEN;

/// Секция в файле бэкапа.
///
/// Свой тип, а не `model::Vault`, нарочно: DTO контракта меняются по договору с
/// фронтом, а формат файла меняться от этого не должен — иначе переименованное
/// ради экрана поле молча перестанет открывать прошлогодний бэкап.
#[derive(Serialize, Deserialize)]
pub struct BackupVault {
    pub vault_id: String,
    pub name: String,
    pub color: String,
    pub sync: bool,
    pub is_default: bool,
    pub created_at: String,
}

/// Запись в файле бэкапа — с распечатанными секретами.
///
/// Шифротекст скопировать нельзя: ключ восстановленного хранилища выводится из
/// той же пары «пароль + соль», но соль у него своя, и AAD полей привязан к
/// `record_id`. Поэтому секреты едут открытым текстом ВНУТРИ зашифрованной
/// полезной нагрузки — то же решение, что у обмена по сети (§3.2).
///
/// `Debug` здесь не выводится вовсе: печатать эту структуру попросту незачем.
#[derive(Serialize, Deserialize)]
pub struct BackupRecord {
    pub record_id: String,
    pub vault_id: String,
    pub service_name: String,
    pub urls: Vec<String>,
    pub login: String,
    pub account_label: Option<String>,
    pub password: String,
    pub notes: Option<String>,
    pub totp_secret: Option<String>,
    /// Версия и все три даты переезжают как есть: восстановленное хранилище —
    /// это то же хранилище, а не его копия с новой историей (§5.2).
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub password_updated_at: String,
}

/// Полезная нагрузка целиком — то, что лежит под шифром.
#[derive(Serialize, Deserialize)]
pub struct Backup {
    pub created_at: String,
    pub vaults: Vec<BackupVault>,
    pub records: Vec<BackupRecord>,
    /// Профиль генератора и настройки безопасности — как они лежат в `meta`,
    /// сырым JSON. Разбирать их здесь незачем: бэкап их только перевозит.
    #[serde(default)]
    pub generator_profile: Option<serde_json::Value>,
    #[serde(default)]
    pub security_settings: Option<serde_json::Value>,
}

/// Собрать файл: вывести ключ из мастер-пароля со свежей солью и запечатать
/// полезную нагрузку заголовком в качестве AAD.
pub fn build(master_password: &str, backup: &Backup) -> CoreResult<Vec<u8>> {
    let salt = crypto::random_salt()?;
    let params = KdfParams::default();
    let header = header(&params, &salt);

    let key = crypto::derive_key(master_password, &salt, &params)?;
    // `Zeroizing`: JSON со всеми паролями хранилища не должен лежать в куче
    // дольше, чем нужно на одно шифрование.
    let plaintext = Zeroizing::new(serde_json::to_vec(backup)?);
    let payload = crypto::seal(&key, &header, &plaintext)?;

    let mut file = Vec::with_capacity(header.len() + payload.len());
    file.extend_from_slice(&header);
    file.extend_from_slice(&payload);
    Ok(file)
}

/// Разобрать файл. Три разных отказа, и путать их нельзя: «это не наш файл»,
/// «файл новее нас» и «пароль не тот» — три разных поступка человека.
pub fn open(master_password: &str, file: &[u8]) -> CoreResult<Backup> {
    if file.len() <= HEADER_LEN || &file[..MAGIC.len()] != MAGIC {
        return Err(CoreError::validation("Это не файл резервной копии Syncra."));
    }

    let mut at = MAGIC.len();
    let version = u16::from_le_bytes([file[at], file[at + 1]]);
    at += 2;
    if version > FORMAT_VERSION {
        return Err(CoreError::validation(
            "Эта резервная копия создана более новой версией Syncra.",
        ));
    }
    if file[at] != KDF_ARGON2ID {
        return Err(CoreError::validation(
            "Эта резервная копия закрыта незнакомым способом.",
        ));
    }
    at += 1;

    let params = KdfParams {
        m_cost: u32_at(file, at),
        t_cost: u32_at(file, at + 4),
        p_cost: u32_at(file, at + 8),
    };
    at += 12;

    // Длина соли читается из файла, но чужая длина означает чужой формат:
    // выводить ключ по ней — значит доверять числу из недоверенного файла.
    if file[at] as usize != SALT_LEN {
        return Err(CoreError::validation("Файл резервной копии повреждён."));
    }
    at += 1;
    let salt = &file[at..at + SALT_LEN];

    let key = crypto::derive_key(master_password, salt, &params)?;
    // Неверный пароль, подменённый заголовок и битый шифротекст здесь
    // неотличимы — как и везде в AEAD. Человеку из этих трёх вероятен один,
    // и говорим мы про него.
    let plaintext = Zeroizing::new(
        crypto::open(&key, &file[..HEADER_LEN], &file[HEADER_LEN..]).map_err(|_| {
            CoreError::new(
                crate::error::CoreErrorCode::InvalidMasterPassword,
                "Эта резервная копия закрыта другим мастер-паролем.",
            )
        })?,
    );

    serde_json::from_slice(&plaintext)
        .map_err(|_| CoreError::validation("Файл резервной копии повреждён."))
}

fn header(params: &KdfParams, salt: &[u8; SALT_LEN]) -> Vec<u8> {
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    header.push(KDF_ARGON2ID);
    header.extend_from_slice(&params.m_cost.to_le_bytes());
    header.extend_from_slice(&params.t_cost.to_le_bytes());
    header.extend_from_slice(&params.p_cost.to_le_bytes());
    header.push(SALT_LEN as u8);
    header.extend_from_slice(salt);
    header
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `unwrap_err` здесь недоступен намеренно: у [`Backup`] нет `Debug`, и
    /// заводить его ради тестов значило бы дать способ напечатать все пароли
    /// хранилища одной строкой.
    #[track_caller]
    fn refusal(result: CoreResult<Backup>) -> CoreError {
        result.err().expect("файл приняли, а не должны были")
    }

    fn sample() -> Backup {
        Backup {
            created_at: "2026-08-23T10:00:00.000Z".to_owned(),
            vaults: vec![BackupVault {
                vault_id: "v-1".to_owned(),
                name: "Личное".to_owned(),
                color: "indigo".to_owned(),
                sync: true,
                is_default: true,
                created_at: "2026-08-23T10:00:00.000Z".to_owned(),
            }],
            records: vec![BackupRecord {
                record_id: "r-1".to_owned(),
                vault_id: "v-1".to_owned(),
                service_name: "GitHub".to_owned(),
                urls: vec!["github.com".to_owned()],
                login: "octocat".to_owned(),
                account_label: None,
                password: "тайна-хранилища".to_owned(),
                notes: None,
                totp_secret: None,
                version: 3,
                created_at: "2026-08-23T10:00:00.000Z".to_owned(),
                updated_at: "2026-08-23T10:00:00.000Z".to_owned(),
                password_updated_at: "2026-08-23T10:00:00.000Z".to_owned(),
            }],
            generator_profile: None,
            security_settings: None,
        }
    }

    #[test]
    fn a_backup_opens_with_its_own_password() {
        let file = build("мастер-пароль-1", &sample()).unwrap();
        let opened = open("мастер-пароль-1", &file).unwrap();

        assert_eq!(opened.records[0].password, "тайна-хранилища");
        assert_eq!(opened.records[0].version, 3);
        assert_eq!(opened.vaults[0].vault_id, "v-1");
    }

    #[test]
    fn the_file_carries_no_plaintext_password() {
        // ЗАКОН №1 в файле: бэкап безопасно хранить в облаке (§6.2).
        let file = build("мастер-пароль-1", &sample()).unwrap();
        let needle = "тайна-хранилища".as_bytes();

        assert!(!file.windows(needle.len()).any(|window| window == needle));
    }

    #[test]
    fn another_password_does_not_open_it() {
        let file = build("мастер-пароль-1", &sample()).unwrap();
        let refused = refusal(open("мастер-пароль-2", &file));

        assert_eq!(
            refused.code,
            crate::error::CoreErrorCode::InvalidMasterPassword
        );
    }

    #[test]
    fn someone_elses_file_is_not_mistaken_for_a_backup() {
        let refused = refusal(open("мастер-пароль-1", b"PK\x03\x04 zip archive, actually"));
        assert_eq!(refused.code, crate::error::CoreErrorCode::Validation);
    }

    #[test]
    fn a_downgraded_kdf_does_not_open_the_payload() {
        // Заголовок входит в AAD: подмена параметров ломает распечатку, а не
        // переводит её на слабые параметры.
        let mut file = build("мастер-пароль-1", &sample()).unwrap();
        file[17] = 1; // m_cost := крошечный
        file[18] = 0;
        file[19] = 0;
        file[20] = 0;

        assert!(open("мастер-пароль-1", &file).is_err());
    }

    #[test]
    fn a_file_from_the_future_says_so() {
        let mut file = build("мастер-пароль-1", &sample()).unwrap();
        file[14] = 99;
        let refused = refusal(open("мастер-пароль-1", &file));

        assert!(
            refused.message.contains("новой версией"),
            "{}",
            refused.message
        );
    }

    #[test]
    fn a_truncated_file_is_refused_and_not_read_past_its_end() {
        let file = build("мастер-пароль-1", &sample()).unwrap();
        for length in [0, 1, MAGIC.len(), HEADER_LEN, HEADER_LEN + 1] {
            assert!(
                open("мастер-пароль-1", &file[..length]).is_err(),
                "длина {length}"
            );
        }
    }
}
