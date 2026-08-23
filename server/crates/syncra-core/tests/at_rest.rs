//! Что именно лежит в файле хранилища (§3.1).
//!
//! Эти тесты проверяют не код, а обещание: ни секретов, ни метаданных в файле не
//! находится, а шифротекст нельзя переставить с места на место. Последний тест
//! держит переход: хранилище, заведённое до S7.1, перекладывает свои открытые
//! метаданные под шифр при первом же отпирании и ничего при этом не теряет.

mod common;

use common::{draft, MASTER_PASSWORD};
use syncra_core::Core;

/// Приметные строки, которых в файле быть не должно.
const PASSWORD: &str = "PLAINTEXT-PASSWORD-MARKER-9f3a";
const NOTES: &str = "PLAINTEXT-NOTES-MARKER-9f3a";
const TOTP: &str = "PLAINTEXT-TOTP-MARKER-9f3a";
const SERVICE: &str = "PLAINTEXT-SERVICE-MARKER-9f3a";
const LOGIN: &str = "PLAINTEXT-LOGIN-MARKER-9f3a";
const URL: &str = "plaintext-url-marker-9f3a.example";
const LABEL: &str = "PLAINTEXT-LABEL-MARKER-9f3a";

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
fn metadata_is_not_readable_in_the_storage_file_either() {
    // До S7.1 здесь стояло обратное утверждение: метаданные лежали открыто, и
    // это была названная граница шага. Теперь `service_name`, `urls` и `login`
    // уезжают в файл одним запечатанным блобом — из `syncra.db` больше не
    // вычитать, к каким сервисам заведены пароли (§3.1).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();
        let mut item = draft(SERVICE, LOGIN, PASSWORD);
        item.urls = vec![URL.to_owned()];
        item.account_label = Some(LABEL.to_owned());
        core.create_record(&item).unwrap();
    }

    let bytes = storage_bytes(dir.path());
    for meta in [SERVICE, LOGIN, URL, LABEL] {
        assert!(
            !contains(&bytes, meta),
            "«{meta}» нашлось в файле хранилища"
        );
    }
}

#[test]
fn a_tombstone_keeps_its_metadata_to_itself() {
    // Надгробие едет по сети наравне с записью (§5.4), поэтому метаданные у него
    // остаются — но остаются ЗАШИФРОВАННЫМИ. Иначе список удалённых сервисов
    // читался бы из файла ещё тридцать дней после удаления.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();
        let created = core
            .create_record(&draft(SERVICE, LOGIN, PASSWORD))
            .unwrap();
        core.delete_record(&created.record_id).unwrap();
    }

    let bytes = storage_bytes(dir.path());
    assert!(!contains(&bytes, SERVICE));
    assert!(!contains(&bytes, PASSWORD));
}

/// Хранилище схемы 5 держало метаданные открытым текстом. Открытие таким
/// хранилищем не должно ни падать, ни терять записи — и обязано переложить их
/// под шифр при первом же отпирании, вместе с отложенной стороной спора.
#[test]
fn a_schema_5_vault_seals_its_metadata_on_the_first_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let record_id = {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();
        core.create_record(&draft(SERVICE, LOGIN, PASSWORD))
            .unwrap()
            .record_id
    };

    // Откатываем файл до схемы 5 мимо ядра — ровно то, что лежит на диске у
    // человека, поставившего прошлую версию.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        common::unseal_metadata(&conn, "records");
        common::unseal_metadata(&conn, "conflicts");
        conn.execute(
            "UPDATE records SET service_name = ?1, urls = ?2, login = ?3, account_label = ?4",
            rusqlite::params![SERVICE, format!("[\"{URL}\"]"), LOGIN, LABEL],
        )
        .unwrap();
        conn.execute_batch(
            "UPDATE meta SET value = CAST('5' AS BLOB) WHERE key = 'schema_version';",
        )
        .unwrap();

        // Пока это схема 5, имя сервиса в файле видно — иначе тест проверял бы
        // не перекладку, а собственный SQL.
        assert!(contains(&storage_bytes(dir.path()), SERVICE));
    }

    {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.unlock(MASTER_PASSWORD).unwrap();

        // Записи целы: метаданные читаются, секреты открываются прежним паролем.
        let list = core.list_records(None, false).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].service_name, SERVICE);
        assert_eq!(list[0].login, LOGIN);
        assert_eq!(list[0].urls, vec![URL.to_owned()]);
        assert_eq!(list[0].account_label.as_deref(), Some(LABEL));
        assert_eq!(core.get_secret(&record_id).unwrap().password, PASSWORD);

        // Перекладка идемпотентна: второе отпирание ничего не ломает.
        core.lock();
        core.unlock(MASTER_PASSWORD).unwrap();
        assert_eq!(core.list_records(None, false).unwrap().len(), 1);
    }

    // ...и открытых колонок в схеме не осталось: колонка, которую забыли
    // перестать заполнять, — это тихий возврат к открытому тексту через полгода.
    let conn = rusqlite::Connection::open(&path).unwrap();
    for table in ["records", "conflicts"] {
        let columns: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}')
                      WHERE name IN ('service_name', 'urls', 'login', 'account_label')"
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(columns, 0, "в {table} остались открытые колонки метаданных");
    }
    drop(conn);

    assert!(!contains(&storage_bytes(dir.path()), SERVICE));
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
