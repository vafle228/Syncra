//! Метаданные записи в файле — одним запечатанным блобом (S7.1, §3.1).
//!
//! # Почему пополе, а не SQLCipher
//!
//! §3.1 обещает шифрование БД, и §4.1 описывает метаданные как «лежащие открыто
//! *внутри* и без того зашифрованной БД». Прямая дорога к этому — SQLCipher, и
//! она закрыта не вкусом: фича `sqlcipher` у `rusqlite` **несовместима с
//! `bundled`** и требует системного `libsqlcipher` с OpenSSL. Это значит внешняя
//! зависимость сборки на каждой машине, где Syncra собирают, — на Windows через
//! vcpkg, — притом что весь остальной проект собирается одним `cargo build`.
//! Платить за at-rest ценой «у половины разработчиков не собирается» нельзя.
//!
//! Поэтому метаданные шифруются так же, как секреты: XChaCha20-Poly1305, ключ
//! хранилища, свой AAD. Результат для файла тот же — из `syncra.db` больше не
//! вычитать, к каким сервисам заведены пароли, — а способ другой.
//!
//! # Чем за это заплачено, честно
//!
//! - **`WHERE service_name LIKE …` и `ORDER BY login` больше не существуют.**
//!   Поиск и сортировка идут в Rust, по расшифрованному списку. Это цена, а не
//!   мелочь: на хранилище в десятки тысяч записей она станет заметной. Сегодня
//!   её платить нечем — `records::list` и так сортировал в Rust, а поиск живёт
//!   на фронте, — но следующий, кому понадобится индекс по метаданным, должен
//!   узнать про это отсюда, а не из профайлера.
//! - **Читать список записей теперь можно только за замком.** Раньше `list`
//!   обходился без ключа; теперь ключ нужен ему так же, как `get_secret`.
//! - **Длина шифротекста выдаёт длину метаданных.** Выравнивания здесь нет —
//!   его нет и у секретов, и заводить его для одного класса значений значило бы
//!   обещать больше, чем ядро делает.
//!
//! # Что НЕ закрыто этим шагом
//!
//! Имена секций (`vaults.name`) и имена устройств (`devices.name`) лежат в файле
//! по-прежнему открыто. Это названная граница, а не недосмотр: «Работа» в списке
//! секций говорит наблюдателю несравнимо меньше, чем список сервисов, а ключ
//! понадобился бы половине запросов, которые сегодня обходятся без него — включая
//! `vaults::adopt` внутри чужого круга обмена.

use serde::{Deserialize, Serialize};

use crate::crypto::{self, VaultKey};
use crate::error::{CoreError, CoreResult};
use crate::model::RecordId;

/// Метаданные записи как они лежат в блобе.
///
/// Секретных полей здесь нет и быть не может — это ровно те четыре, что раньше
/// лежали в файле открытым текстом. Порядок и имена полей — часть формата: их
/// читает `serde_json` из БД, заведённой прошлой версией.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordFields {
    pub service_name: String,
    pub urls: Vec<String>,
    pub login: String,
    /// `Option`, а не пустая строка: «подписи нет» и «подпись пустая» — разные
    /// вещи для карточки записи (§4.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
}

/// Где лежит блоб. От этого зависит AAD — и только он.
///
/// Две таблицы, а не одна, потому что приехавшая версия спора хранит свои
/// метаданные отдельно от местных (§5.5). С общим AAD их шифротексты стали бы
/// взаимозаменяемы, и тот, кто может писать в файл, подменил бы имя сервиса
/// живой записи именем из спора.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Record,
    Conflict,
}

impl Place {
    fn aad(self, record_id: &str) -> Vec<u8> {
        match self {
            Place::Record => crypto::record_meta_aad(record_id),
            Place::Conflict => crypto::conflict_meta_aad(record_id),
        }
    }
}

pub fn seal(
    key: &VaultKey,
    place: Place,
    record_id: &RecordId,
    fields: &RecordFields,
) -> CoreResult<Vec<u8>> {
    let json = serde_json::to_vec(fields)?;
    crypto::seal(key, &place.aad(record_id), &json)
}

/// Распечатать метаданные.
///
/// `None` вместо блоба — это **испорченный файл**, а не «ещё не заполнено».
/// Столбец заведён допускающим NULL только потому, что `ALTER TABLE ADD COLUMN`
/// иначе не умеет; заполняется он в той же транзакции, что и вставка строки, а у
/// хранилищ прошлых схем — при первом отпирании (`schema::seal_legacy_metadata`).
/// Пустой блоб после этого означает, что в файл писал кто-то посторонний.
pub fn open(
    key: &VaultKey,
    place: Place,
    record_id: &RecordId,
    blob: Option<&[u8]>,
) -> CoreResult<RecordFields> {
    let blob = blob.ok_or_else(|| CoreError::internal("Хранилище повреждено."))?;
    let json = crypto::open(key, &place.aad(record_id), blob)?;
    serde_json::from_slice(&json)
        .map_err(|_| CoreError::internal("Зашифрованное значение повреждено."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    fn key(byte: u8) -> VaultKey {
        Zeroizing::new([byte; crypto::KEY_LEN])
    }

    fn fields() -> RecordFields {
        RecordFields {
            service_name: "GitHub".to_owned(),
            urls: vec!["github.com".to_owned(), "gist.github.com".to_owned()],
            login: "octocat".to_owned(),
            account_label: Some("рабочий".to_owned()),
        }
    }

    #[test]
    fn metadata_survives_a_seal_and_open() {
        let blob = seal(&key(7), Place::Record, &"r1".to_owned(), &fields()).unwrap();
        let back = open(&key(7), Place::Record, &"r1".to_owned(), Some(&blob)).unwrap();
        assert_eq!(back, fields());
    }

    #[test]
    fn a_blob_does_not_contain_the_service_name() {
        let blob = seal(&key(7), Place::Record, &"r1".to_owned(), &fields()).unwrap();
        assert!(!blob.windows(6).any(|window| window == b"GitHub"));
    }

    #[test]
    fn a_blob_belongs_to_its_record_and_its_table() {
        let blob = seal(&key(7), Place::Record, &"r1".to_owned(), &fields()).unwrap();

        // Переставить блоб в соседнюю запись...
        assert!(open(&key(7), Place::Record, &"r2".to_owned(), Some(&blob)).is_err());
        // ...или на место приехавшей версии спора — не выйдет ни то, ни другое.
        assert!(open(&key(7), Place::Conflict, &"r1".to_owned(), Some(&blob)).is_err());
        // Чужой ключ хранилища — тем более.
        assert!(open(&key(8), Place::Record, &"r1".to_owned(), Some(&blob)).is_err());
    }

    #[test]
    fn a_missing_blob_is_a_broken_file_not_an_empty_record() {
        let error = open(&key(7), Place::Record, &"r1".to_owned(), None).unwrap_err();
        assert!(!error.message.is_empty());
    }

    #[test]
    fn a_record_without_a_label_round_trips_as_none() {
        let bare = RecordFields {
            account_label: None,
            ..fields()
        };
        let blob = seal(&key(7), Place::Record, &"r1".to_owned(), &bare).unwrap();
        assert_eq!(
            open(&key(7), Place::Record, &"r1".to_owned(), Some(&blob)).unwrap(),
            bare
        );
    }
}
