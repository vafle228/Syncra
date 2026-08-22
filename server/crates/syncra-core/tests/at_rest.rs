//! Что именно лежит в файле хранилища (§3.1).
//!
//! Эти тесты проверяют не код, а обещание: секреты в файле не находятся, а
//! шифротекст нельзя переставить с места на место. Второй тест заодно
//! документирует принятую границу шага: метаданные в файле лежат ОТКРЫТО.

mod common;

use common::{draft, MASTER_PASSWORD};
use syncra_core::Core;

/// Приметные строки, которых в файле быть не должно.
const PASSWORD: &str = "PLAINTEXT-PASSWORD-MARKER-9f3a";
const NOTES: &str = "PLAINTEXT-NOTES-MARKER-9f3a";
const TOTP: &str = "PLAINTEXT-TOTP-MARKER-9f3a";
const SERVICE: &str = "PLAINTEXT-SERVICE-MARKER-9f3a";

/// Все файлы хранилища одним куском: при WAL часть данных живёт в `-wal`, и
/// смотреть только на `.db` значило бы проверять пустое место.
fn storage_bytes(dir: &std::path::Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            bytes.extend_from_slice(&std::fs::read(&path).unwrap());
        }
    }
    bytes
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[test]
fn secrets_are_not_readable_in_the_storage_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();

        let mut item = draft(SERVICE, "octocat", PASSWORD);
        item.notes = Some(NOTES.to_owned());
        item.totp_secret = Some(TOTP.to_owned());
        core.create_record(&item).unwrap();
    } // соединение закрыто, WAL сведён в основной файл

    let bytes = storage_bytes(dir.path());

    assert!(
        !contains(&bytes, PASSWORD),
        "пароль нашёлся в файле хранилища"
    );
    assert!(
        !contains(&bytes, NOTES),
        "заметка нашлась в файле хранилища"
    );
    assert!(
        !contains(&bytes, TOTP),
        "ключ TOTP нашёлся в файле хранилища"
    );
    assert!(
        !contains(&bytes, MASTER_PASSWORD),
        "мастер-пароль нашёлся в файле хранилища"
    );
}

#[test]
fn rekeyed_storage_keeps_its_secrets_to_itself() {
    // Перешифровка (F13) переписывает каждый шифротекст: если бы она хоть где-то
    // оставила открытый текст «на минуточку», файл бы это показал.
    const NEW_MASTER_PASSWORD: &str = "мастер-пароль-2-подлиннее";

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();

        let mut item = draft(SERVICE, "octocat", PASSWORD);
        item.notes = Some(NOTES.to_owned());
        item.totp_secret = Some(TOTP.to_owned());
        core.create_record(&item).unwrap();

        core.change_master_password(MASTER_PASSWORD, NEW_MASTER_PASSWORD)
            .unwrap();
    }

    let bytes = storage_bytes(dir.path());
    for secret in [PASSWORD, NOTES, TOTP, MASTER_PASSWORD, NEW_MASTER_PASSWORD] {
        assert!(
            !contains(&bytes, secret),
            "«{secret}» нашлось в файле хранилища после смены пароля"
        );
    }
}

#[test]
fn metadata_is_deliberately_not_encrypted_in_this_step() {
    // Пополевое шифрование закрывает секреты, но НЕ метаданные: имя сервиса и
    // логин лежат в файле открыто. Это принятая граница шага, а не недосмотр, и
    // тест держит её видимой — когда появится шифрование файла целиком, он
    // упадёт и потребует переписать себя вместе с обещанием.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();
        core.create_record(&draft(SERVICE, "octocat", PASSWORD))
            .unwrap();
    }

    assert!(contains(&storage_bytes(dir.path()), SERVICE));
}

#[test]
fn ciphertext_cannot_be_moved_between_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let (donor, victim) = {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();
        let donor = core
            .create_record(&draft("Donor", "donor", "чужой-пароль"))
            .unwrap();
        let victim = core
            .create_record(&draft("Victim", "victim", "свой-пароль"))
            .unwrap();
        (donor.record_id, victim.record_id)
    };

    // Тот, кто может писать в файл БД, переставляет чужой шифротекст пароля в
    // свою запись. Без AAD это сработало бы: сам по себе шифротекст не знает,
    // где он лежит.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE records SET password_ct = (SELECT password_ct FROM records WHERE record_id = ?1)
             WHERE record_id = ?2",
            rusqlite::params![donor, victim],
        )
        .unwrap();
    }

    let mut core = Core::open(&path, common::host()).unwrap();
    core.unlock(MASTER_PASSWORD).unwrap();

    // Подменённая запись не открывается вовсе — вместо того чтобы отдать чужой пароль.
    assert!(core.get_secret(&victim).is_err());
    // Соседняя запись при этом цела.
    assert_eq!(core.get_secret(&donor).unwrap().password, "чужой-пароль");
}
