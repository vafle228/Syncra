//! Идентичность устройства и таблица доверия (F9, §2.1 · §2.3).
//!
//! Проверяем обещания, на которых стоит всё остальное: у устройства есть пара
//! ключей, она переживает и запирание, и смену мастер-пароля, себя отозвать
//! нельзя, а момент отзыва повторным нажатием не переписывается.

use std::path::Path;

use syncra_core::{Core, CoreErrorCode, Device};

mod common;

const NEW_MASTER_PASSWORD: &str = "мастер-пароль-2-подлиннее";

/// Второе устройство в `devices`. Кладётся в файл прямо, мимо ядра, — тем же
/// приёмом, что `at_rest.rs` смотрит в хранилище: здесь проверяется отзыв, а не
/// то, как устройство туда попало. Настоящее сопряжение через `confirm_pairing`
/// разбирает `tests/pairing.rs`, и повторять его тут значило бы поставить тесты
/// отзыва в зависимость от работы сопряжения.
const PEER_ID: &str = "peer-device-1";

fn insert_peer(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(
        "INSERT INTO devices
           (device_id, name, kind, public_key, fingerprint_words,
            paired_at, last_seen_at, revoked_at)
         VALUES (?1, 'ThinkPad X1', 'desktop', X'0102', '[\"якорь\"]',
                 '2026-01-01T00:00:00.000Z', NULL, NULL)",
        [PEER_ID],
    )
    .unwrap();
}

/// Публичный ключ и запечатанный секрет так, как их видит посторонний с файлом.
fn stored_identity(path: &Path) -> (Vec<u8>, Vec<u8>) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let device_id: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'device_id'",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map(|bytes| String::from_utf8(bytes).unwrap())
        .unwrap();
    let public_key: Vec<u8> = conn
        .query_row(
            "SELECT public_key FROM devices WHERE device_id = ?1",
            [&device_id],
            |row| row.get(0),
        )
        .unwrap();
    let secret: Vec<u8> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'device_secret'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    (public_key, secret)
}

fn this_device(core: &Core) -> Device {
    core.list_devices()
        .unwrap()
        .into_iter()
        .find(|device| device.is_this_device)
        .expect("своё устройство в списке")
}

// ---------------------------------------------------------------------------
// Идентичность
// ---------------------------------------------------------------------------

#[test]
fn a_fresh_vault_knows_exactly_one_device_and_it_is_this_one() {
    let core = common::unlocked();

    let devices = core.list_devices().unwrap();
    assert_eq!(devices.len(), 1);

    let device = &devices[0];
    assert!(device.is_this_device);
    assert_eq!(device.name, common::HOST_NAME, "имя приходит от оболочки");
    assert_eq!(device.revoked_at, None);
    assert_eq!(
        device.fingerprint_words.len(),
        4,
        "четыре слова — столько сверяет человек (§3.6)"
    );
    // Отпечаток — слова, а не ключ: пустых строк среди них быть не должно.
    assert!(device.fingerprint_words.iter().all(|word| !word.is_empty()));
}

#[test]
fn the_device_list_is_behind_the_lock() {
    let mut core = common::unlocked();
    core.lock();

    common::assert_code(core.list_devices(), CoreErrorCode::Locked);
    common::assert_code(core.revoke_device(PEER_ID), CoreErrorCode::Locked);
}

#[test]
fn the_key_survives_lock_and_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let before = {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(common::MASTER_PASSWORD).unwrap();
        let device = this_device(&core);
        core.lock();
        core.unlock(common::MASTER_PASSWORD).unwrap();

        // Отпирание не заводит устройству новую пару и не переписывает старую.
        assert_eq!(this_device(&core), device);
        stored_identity(&path)
    };

    // ...и после перезапуска процесса тоже.
    let mut core = Core::open(&path, common::host()).unwrap();
    core.unlock(common::MASTER_PASSWORD).unwrap();
    assert_eq!(stored_identity(&path).0, before.0);
}

#[test]
fn the_key_survives_a_master_password_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let mut core = Core::open(&path, common::host()).unwrap();
    core.init_vault(common::MASTER_PASSWORD).unwrap();

    let device = this_device(&core);
    let (public_before, sealed_before) = stored_identity(&path);

    core.change_master_password(common::MASTER_PASSWORD, NEW_MASTER_PASSWORD)
        .unwrap();

    let (public_after, sealed_after) = stored_identity(&path);
    // Идентичность устройства — публичный ключ; он не меняется от того, что
    // владелец поменял пароль.
    assert_eq!(public_after, public_before);
    assert_eq!(this_device(&core), device);
    // А вот шифротекст обязан быть другим: старым ключом его больше не открыть.
    assert_ne!(sealed_after, sealed_before);

    // И самое главное: после перезапуска ключ открывается НОВЫМ паролем.
    core.lock();
    core.unlock(NEW_MASTER_PASSWORD).unwrap();
    assert_eq!(this_device(&core), device);
    assert_eq!(stored_identity(&path).0, public_before);
}

#[test]
fn changing_the_password_counts_the_devices_that_have_to_catch_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let mut core = Core::open(&path, common::host()).unwrap();
    core.init_vault(common::MASTER_PASSWORD).unwrap();
    insert_peer(&path);

    let response = core
        .change_master_password(common::MASTER_PASSWORD, NEW_MASTER_PASSWORD)
        .unwrap();

    // Своё устройство в счёт не идёт: оно уже обновилось.
    assert_eq!(response.devices_to_update, 1);
}

// ---------------------------------------------------------------------------
// Отзыв (§2.3)
// ---------------------------------------------------------------------------

#[test]
fn this_device_cannot_be_revoked() {
    let core = common::unlocked();
    let device = this_device(&core);

    // Устройство в руках — единственное, что у человека точно есть.
    common::assert_code(
        core.revoke_device(&device.device_id),
        CoreErrorCode::Validation,
    );
    assert_eq!(this_device(&core).revoked_at, None);
}

#[test]
fn revoking_an_unknown_device_is_not_found() {
    let core = common::unlocked();

    common::assert_code(core.revoke_device("нет-такого"), CoreErrorCode::NotFound);
}

#[test]
fn revoking_twice_keeps_the_first_moment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let mut core = Core::open(&path, common::host()).unwrap();
    core.init_vault(common::MASTER_PASSWORD).unwrap();
    insert_peer(&path);

    let first = core.revoke_device(PEER_ID).unwrap();
    assert!(first.revoked_at.is_some());
    assert!(!first.is_this_device);

    // Момент отзыва — факт, от которого считается, что успело разойтись по сети.
    // Повторное нажатие его не переписывает.
    assert_eq!(core.revoke_device(PEER_ID).unwrap(), first);
}

#[test]
fn a_revoked_device_stays_in_the_list_and_leaves_its_neighbours_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let mut core = Core::open(&path, common::host()).unwrap();
    core.init_vault(common::MASTER_PASSWORD).unwrap();
    insert_peer(&path);

    let before = core.list_devices().unwrap();
    let revoked = core.revoke_device(PEER_ID).unwrap();

    core.lock();
    core.unlock(common::MASTER_PASSWORD).unwrap();
    let after = core.list_devices().unwrap();

    // Отзыв — не удаление строки: человек должен видеть, что ноутбук отозван,
    // а не что его никогда не было.
    assert_eq!(after.len(), before.len());
    assert_eq!(
        after.iter().find(|d| d.device_id == PEER_ID),
        Some(&revoked)
    );
    // Своё устройство идёт первым и отзыва соседа не заметило.
    assert_eq!(after[0], before[0]);
    assert!(after[0].is_this_device);
}

// ---------------------------------------------------------------------------
// Миграция схемы 1 → 2
// ---------------------------------------------------------------------------

/// Хранилище схемы 1 создавалось без таблицы `devices` и без пары ключей.
/// Открытие таким хранилищем не должно ни падать, ни терять записи — и должно
/// доснабдить устройство идентичностью при первом же отпирании.
#[test]
fn a_schema_1_vault_migrates_without_losing_anything() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let record_id = {
        let mut core = Core::open(&path, common::host()).unwrap();
        core.init_vault(common::MASTER_PASSWORD).unwrap();
        core.create_record(&common::draft("GitHub", "octocat", "тайна-1"))
            .unwrap()
            .record_id
    };

    // Откатываем файл до схемы 1 мимо ядра — ровно то, что лежит на диске у
    // человека, поставившего прошлую версию: ни доверенных устройств, ни пары
    // ключей, ни отметок обмена, а метаданные записи лежат открыто.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE devices;
             DROP TABLE conflicts;
             DROP TABLE sync_state;
             DELETE FROM meta WHERE key = 'device_secret';",
        )
        .unwrap();
        common::unseal_metadata(&conn, "records");
        conn.execute_batch(
            "UPDATE records SET service_name = 'GitHub', urls = '[\"github.com\"]',
                                login = 'octocat';
             UPDATE meta SET value = CAST('1' AS BLOB) WHERE key = 'schema_version';",
        )
        .unwrap();
    }

    let mut core = Core::open(&path, common::host()).unwrap();
    core.unlock(common::MASTER_PASSWORD).unwrap();

    // Записи целы, секреты открываются прежним паролем.
    assert_eq!(core.list_records(None, false).unwrap().len(), 1);
    assert_eq!(
        core.list_records(None, false).unwrap()[0].service_name,
        "GitHub"
    );
    assert_eq!(core.get_secret(&record_id).unwrap().password, "тайна-1");

    // ...и устройство завелось: без этого мигрированное хранилище навсегда
    // осталось бы без права представляться собой.
    let device = this_device(&core);
    assert_eq!(device.name, common::HOST_NAME);
    assert_eq!(device.fingerprint_words.len(), 4);

    // Досоздание идемпотентно: второе отпирание пару не пересоздаёт.
    let public_key = stored_identity(&path).0;
    core.lock();
    core.unlock(common::MASTER_PASSWORD).unwrap();
    assert_eq!(stored_identity(&path).0, public_key);
}
