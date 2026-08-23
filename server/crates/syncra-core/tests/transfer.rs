//! Перенос данных: CSV, зашифрованный бэкап и импорт (F12, §6.2 · §6.3).
//!
//! Зеркалит `client/src/core/__tests__/transfer.spec.ts`.
//!
//! Файлы здесь настоящие: экспорт пишет на диск, импорт читает с диска, а
//! восстановление поднимает второе хранилище из файла. Временную папку даёт
//! `tempfile` — как в `lifecycle.rs::vault_survives_restart`.

mod common;

use std::path::{Path, PathBuf};

use common::{assert_code, draft, host, unlocked, MASTER_PASSWORD};
use syncra_core::{Core, CoreErrorCode, ImportOptions, ImportRowStatus, ImportSource};

/// Пароли, по которым проверяется Закон №1: их ищут в байтах созданных файлов.
const PASSWORD: &str = "пароль-от-гитхаба";
const NOTE: &str = "заметка-с-кодами";

fn options(skip_duplicates: bool, flag_reused: bool) -> ImportOptions {
    ImportOptions {
        skip_duplicates,
        flag_reused,
    }
}

/// Хранилище с парой записей — то, что человек и понесёт в другой менеджер.
fn filled() -> Core {
    let core = unlocked();
    let mut first = draft("GitHub", "octocat", PASSWORD);
    first.notes = Some(NOTE.to_owned());
    first.totp_secret = Some("JBSWY3DPEHPK3PXP".to_owned());
    core.create_record(&first).expect("первая запись");
    core.create_record(&draft("Ozon", "demo@example.com", "пароль-от-озона"))
        .expect("вторая запись");
    core
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("файл-образец");
    path
}

/// Экспорт Chrome: пять колонок, одна строка-дубликат, одна без пароля.
const CHROME_CSV: &str = "name,url,username,password,note\n\
                          Figma,https://figma.com/,demo@example.com,пароль-фигмы,\n\
                          GitHub,https://github.com/,octocat,другой-пароль,\n\
                          Форум,https://forum.example/,demo,,\n\
                          Ozon,https://ozon.ru/,another,пароль-фигмы,\n";

// ---------------------------------------------------------------------------
// Экспорт
// ---------------------------------------------------------------------------

#[test]
fn a_csv_with_the_wrong_master_password_creates_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = filled();

    assert_code(
        core.export_csv("не-тот-пароль", dir.path()),
        CoreErrorCode::InvalidMasterPassword,
    );

    // Ни файла, ни следа в реестре: отказ должен быть отказом целиком.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn a_csv_carries_every_live_record_in_the_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = filled();

    let file = core.export_csv(MASTER_PASSWORD, dir.path()).unwrap();

    assert_eq!(file.record_count, 2);
    assert!(!file.encrypted, "CSV не шифруется — и говорит об этом");
    assert!(file.file_name.starts_with("syncra-plain-"));
    assert!(file.file_name.ends_with(".csv"));

    // Открытый текст здесь — это НЕ утечка, а смысл файла (§6.2). Тест держит
    // обещание видимым: молча зашифровать его нельзя, не переписав обещание.
    let text = std::fs::read_to_string(&file.path).unwrap();
    assert!(text.contains(PASSWORD), "пароля нет в файле для переезда");
    assert!(text.contains(NOTE));
    assert!(text.contains("JBSWY3DPEHPK3PXP"), "ключ TOTP потерян");
    assert_eq!(file.size_bytes, text.len() as i64);
}

#[test]
fn a_backup_is_closed_and_carries_no_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = filled();

    let file = core.export_backup(MASTER_PASSWORD, dir.path()).unwrap();

    assert!(file.encrypted);
    assert!(file.file_name.ends_with(".syncra"));

    // ЗАКОН №1 в файле: бэкап безопасно хранить в облаке (§6.2).
    let bytes = std::fs::read(&file.path).unwrap();
    for secret in [PASSWORD, NOTE] {
        let needle = secret.as_bytes();
        assert!(
            !bytes.windows(needle.len()).any(|window| window == needle),
            "секрет лежит в бэкапе открытым текстом"
        );
    }
}

#[test]
fn exporting_twice_in_a_day_replaces_the_file_instead_of_piling_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = filled();

    let first = core.export_csv(MASTER_PASSWORD, dir.path()).unwrap();
    let second = core.export_csv(MASTER_PASSWORD, dir.path()).unwrap();

    assert_eq!(first.path, second.path);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    // И в реестре он тоже один: удалить надо будет один файл, а не два.
    core.delete_export(&second.path).expect("удаление");
    assert_code(core.delete_export(&first.path), CoreErrorCode::NotFound);
}

#[test]
fn a_file_that_cannot_be_written_says_so_about_the_file_not_the_network() {
    let dir = tempfile::tempdir().unwrap();
    // Папка экспорта — на самом деле файл: создать в ней ничего нельзя.
    let blocked = write(dir.path(), "занято", "не папка");
    let mut core = filled();

    let refused = core
        .export_csv(MASTER_PASSWORD, &blocked)
        .expect_err("запись в файл как в папку удалась");

    // `From<io::Error>` в ядре рассказывает про СЕТЬ — здесь это была бы ложь.
    assert!(
        !refused.message.contains("устройством в сети"),
        "файловая ошибка притворилась сетевой: {}",
        refused.message
    );
}

// ---------------------------------------------------------------------------
// Удаление созданного файла
// ---------------------------------------------------------------------------

#[test]
fn delete_export_refuses_a_path_it_did_not_create() {
    let dir = tempfile::tempdir().unwrap();
    let stranger = write(dir.path(), "чужой.txt", "чужие данные");
    let mut core = filled();

    assert_code(
        core.delete_export(&stranger.to_string_lossy()),
        CoreErrorCode::NotFound,
    );
    // Команда, которой можно передать любой путь, была бы способом стереть с
    // диска что угодно чужими руками. Файл обязан остаться на месте.
    assert!(stranger.exists(), "чужой файл всё-таки удалили");
}

#[test]
fn delete_export_removes_its_own_file_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = filled();

    let file = core.export_csv(MASTER_PASSWORD, dir.path()).unwrap();
    core.delete_export(&file.path).expect("удаление");

    assert!(!Path::new(&file.path).exists());
    // Повторное удаление — уже не «удалить», а «удалить неизвестно что».
    assert_code(core.delete_export(&file.path), CoreErrorCode::NotFound);
}

// ---------------------------------------------------------------------------
// Восстановление
// ---------------------------------------------------------------------------

#[test]
fn a_backup_restores_into_an_identical_vault() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = filled();
    // Секция, которая НЕ синхронизируется: её флаг обязан пережить переезд.
    let local = source.create_vault("Локальное", "coral").unwrap();
    source.set_vault_sync(&local.vault_id, false).unwrap();

    let file = source.export_backup(MASTER_PASSWORD, dir.path()).unwrap();
    let before = source.list_records(None, false).unwrap();

    // Чистая машина: своё хранилище, своя папка данных.
    let fresh_dir = tempfile::tempdir().unwrap();
    let mut restored = Core::open(&fresh_dir.path().join("syncra.db"), host()).unwrap();
    let result = restored
        .restore_backup(MASTER_PASSWORD, Path::new(&file.path))
        .unwrap();

    assert_eq!(result.records, before.len() as i64);
    assert_eq!(result.vaults, 2);
    assert_eq!(result.file_name, file.file_name);
    // Хранилище открыто: пароль только что вводили.
    assert!(restored.status().unwrap().unlocked);

    let after = restored.list_records(None, false).unwrap();
    assert_eq!(after, before, "запись переехала не такой, какой была");
    for record in &after {
        let secrets = restored.get_secret(&record.record_id).unwrap();
        assert_eq!(
            secrets.password,
            source.get_secret(&record.record_id).unwrap().password
        );
    }

    let restored_local = restored
        .list_vaults()
        .unwrap()
        .into_iter()
        .find(|vault| vault.vault_id == local.vault_id)
        .expect("локальная секция переехала со своим идентификатором");
    assert!(!restored_local.sync, "секция приехала синкаемой");
}

#[test]
fn a_restored_vault_is_a_new_device() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = filled();
    let file = source.export_backup(MASTER_PASSWORD, dir.path()).unwrap();
    let source_id = only_device(&source);

    let fresh_dir = tempfile::tempdir().unwrap();
    let mut restored = Core::open(&fresh_dir.path().join("syncra.db"), host()).unwrap();
    restored
        .restore_backup(MASTER_PASSWORD, Path::new(&file.path))
        .unwrap();

    // §2.1: идентичность в бэкап не едет. Иначе две машины получили бы право
    // представляться одним устройством, и отзыв одной убивал бы обе.
    assert_ne!(only_device(&restored), source_id);
}

fn only_device(core: &Core) -> String {
    core.list_devices()
        .unwrap()
        .into_iter()
        .find(|device| device.is_this_device)
        .expect("своё устройство всегда в списке")
        .device_id
}

#[test]
fn restoring_over_a_live_vault_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = filled();
    let file = source.export_backup(MASTER_PASSWORD, dir.path()).unwrap();

    // Восстановление поверх живого — это слияние двух историй, то есть задача
    // синхронизации с конфликтами, а не «перезаписать файлом с флешки».
    assert_code(
        source.restore_backup(MASTER_PASSWORD, Path::new(&file.path)),
        CoreErrorCode::AlreadyInitialized,
    );
}

#[test]
fn a_backup_opened_with_another_password_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = filled();
    let file = source.export_backup(MASTER_PASSWORD, dir.path()).unwrap();

    let fresh_dir = tempfile::tempdir().unwrap();
    let mut restored = Core::open(&fresh_dir.path().join("syncra.db"), host()).unwrap();

    assert_code(
        restored.restore_backup("совсем-другой-пароль", Path::new(&file.path)),
        CoreErrorCode::InvalidMasterPassword,
    );
    // Неудачное восстановление не создаёт хранилища: иначе второй попытки не
    // будет — она упрётся в `ALREADY_INITIALIZED`.
    assert!(!restored.status().unwrap().initialized);
}

#[test]
fn someone_elses_file_is_not_mistaken_for_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let stranger = write(dir.path(), "отпуск.jpg", "не бэкап, а фотография");

    let fresh_dir = tempfile::tempdir().unwrap();
    let mut restored = Core::open(&fresh_dir.path().join("syncra.db"), host()).unwrap();

    assert_code(
        restored.restore_backup(MASTER_PASSWORD, &stranger),
        CoreErrorCode::Validation,
    );
}

// ---------------------------------------------------------------------------
// Импорт
// ---------------------------------------------------------------------------

#[test]
fn the_preview_carries_no_password() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "Chrome Passwords.csv", CHROME_CSV);
    let mut core = filled();

    let preview = core.begin_import(ImportSource::Chrome, &file).unwrap();
    let wire = serde_json::to_string(&preview).unwrap();

    assert!(!wire.contains("пароль-фигмы"), "{wire}");
    assert!(!wire.contains("другой-пароль"), "{wire}");
    // При этом видно, что попадёт внутрь: адрес, логин и что с ним будет.
    assert!(wire.contains("figma.com"));
    assert_eq!(preview.source, ImportSource::Chrome);
    assert_eq!(preview.file_name, "Chrome Passwords.csv");
}

#[test]
fn the_preview_counts_the_whole_file_and_shows_the_first_rows() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "passwords.csv", CHROME_CSV);
    let mut core = filled();

    let preview = core.begin_import(ImportSource::Csv, &file).unwrap();

    assert_eq!(preview.total_rows, 4);
    // `github.com / octocat` уже есть в хранилище — это дубликат; у форума в
    // файле нет пароля; остальные две — новые.
    assert_eq!(preview.new_count, 2);
    assert_eq!(preview.duplicate_count, 1);
    assert_eq!(preview.no_password_count, 1);
    assert_eq!(preview.rows.len(), 4);
    assert_eq!(preview.rows[1].status, ImportRowStatus::Duplicate);
    assert_eq!(preview.rows[2].status, ImportRowStatus::NoPassword);
    assert!(preview.target_vault_name.starts_with("Импорт "));
}

#[test]
fn a_commit_puts_everything_in_its_own_section_and_deletes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "Chrome Passwords.csv", CHROME_CSV);
    let mut core = filled();

    let preview = core.begin_import(ImportSource::Chrome, &file).unwrap();
    let result = core
        .commit_import(&preview.session_id, &options(true, true))
        .unwrap();

    assert_eq!(result.imported, 2, "дубликат и строка без пароля не едут");
    assert_eq!(result.skipped, 1, "в пропущенных только дубликат");
    assert_eq!(result.vault.name, preview.target_vault_name);
    assert!(result.vault.sync, "импортированное синхронизируется");
    assert!(!result.vault.is_default);
    // Обещание §3.10 макета: файл разбирается здесь и удаляется сразу.
    assert!(result.source_file_deleted);
    assert!(!file.exists());
    // Два сайта делят один пароль — это и есть наследство прошлого менеджера.
    assert_eq!(result.reused_passwords, 2);

    let imported = core
        .list_records(Some(&result.vault.vault_id), false)
        .unwrap();
    assert_eq!(imported.len(), 2);
    let figma = imported
        .iter()
        .find(|record| record.service_name == "Figma")
        .expect("имя сервиса собрано из адреса");
    assert_eq!(figma.urls, vec!["figma.com".to_owned()]);
    assert_eq!(figma.version, 1);
    assert_eq!(
        core.get_secret(&figma.record_id).unwrap().password,
        "пароль-фигмы"
    );
}

#[test]
fn keeping_duplicates_brings_the_second_account_in() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "passwords.csv", CHROME_CSV);
    let mut core = filled();

    let preview = core.begin_import(ImportSource::Csv, &file).unwrap();
    let result = core
        .commit_import(&preview.session_id, &options(false, false))
        .unwrap();

    // Два аккаунта на один сервис — норма, а не коллизия (§4.4).
    assert_eq!(result.imported, 3);
    assert_eq!(result.skipped, 0);
    // Не просили считать повторы — значит ноль, а не «посчитали на всякий случай».
    assert_eq!(result.reused_passwords, 0);
}

#[test]
fn a_second_import_the_same_day_lands_in_the_same_section() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = filled();

    let mut import = |name: &str| {
        let file = write(dir.path(), name, CHROME_CSV);
        let preview = core.begin_import(ImportSource::Csv, &file).unwrap();
        core.commit_import(&preview.session_id, &options(false, false))
            .unwrap()
            .vault
    };

    assert_eq!(import("первый.csv").vault_id, import("второй.csv").vault_id);
}

#[test]
fn a_cancelled_import_leaves_no_parsed_rows() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "Chrome Passwords.csv", CHROME_CSV);
    let mut core = filled();

    let preview = core.begin_import(ImportSource::Chrome, &file).unwrap();
    core.cancel_import(&preview.session_id).expect("отмена");
    // Идемпотентно: по «Отмене» нажимают дважды.
    core.cancel_import(&preview.session_id)
        .expect("вторая отмена");

    assert_code(
        core.commit_import(&preview.session_id, &options(true, true)),
        CoreErrorCode::NotFound,
    );
    // Отказались — значит ничего не завелось и файл на месте: удаляет его
    // подтверждение, а не разбор.
    assert_eq!(core.list_records(None, false).unwrap().len(), 2);
    assert!(file.exists());
}

#[test]
fn a_session_is_good_for_exactly_one_commit() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "passwords.csv", CHROME_CSV);
    let mut core = filled();

    let preview = core.begin_import(ImportSource::Csv, &file).unwrap();
    core.commit_import(&preview.session_id, &options(true, true))
        .unwrap();

    assert_code(
        core.commit_import(&preview.session_id, &options(true, true)),
        CoreErrorCode::NotFound,
    );
}

#[test]
fn a_lock_forgets_parsed_files_but_not_created_ones() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "passwords.csv", CHROME_CSV);
    let mut core = filled();

    let export = core.export_csv(MASTER_PASSWORD, dir.path()).unwrap();
    let preview = core.begin_import(ImportSource::Csv, &file).unwrap();

    core.lock();
    core.unlock(MASTER_PASSWORD).unwrap();

    // Чужие пароли замок не переживают.
    assert_code(
        core.commit_import(&preview.session_id, &options(true, true)),
        CoreErrorCode::NotFound,
    );
    // А созданный файл — переживает: замок не умеет стереть его с диска, и
    // делать вид, что умеет, не будет.
    core.delete_export(&export.path)
        .expect("файл всё ещё числится за ядром");
}

#[test]
fn a_keepass_database_says_what_to_do_instead() {
    let dir = tempfile::tempdir().unwrap();
    // ГРАНИЦА ШАГА: .kdbx всегда зашифрован, а `begin_import` в контракте не
    // принимает пароля. Пока контракт такой — это честный отказ, а не молчание.
    let file = write(dir.path(), "Passwords.kdbx", "содержимое базы");
    let mut core = filled();

    let refused = core
        .begin_import(ImportSource::KeePass, &file)
        .expect_err(".kdbx приняли, хотя открыть его нечем");

    assert_eq!(refused.code, CoreErrorCode::Validation);
    assert!(refused.message.contains("CSV"), "{}", refused.message);
}

#[test]
fn a_table_that_is_not_an_export_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "бюджет.csv", "месяц,сумма\nмай,1000\n");
    let mut core = filled();

    assert_code(
        core.begin_import(ImportSource::Csv, &file),
        CoreErrorCode::Validation,
    );
}

// ---------------------------------------------------------------------------
// Замок
// ---------------------------------------------------------------------------

#[test]
fn a_locked_vault_answers_locked_to_all_of_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "passwords.csv", CHROME_CSV);
    let mut core = filled();
    core.lock();

    assert_code(
        core.export_csv(MASTER_PASSWORD, dir.path()),
        CoreErrorCode::Locked,
    );
    assert_code(
        core.export_backup(MASTER_PASSWORD, dir.path()),
        CoreErrorCode::Locked,
    );
    assert_code(core.delete_export("что-угодно"), CoreErrorCode::Locked);
    assert_code(
        core.begin_import(ImportSource::Csv, &file),
        CoreErrorCode::Locked,
    );
    assert_code(
        core.commit_import("что-угодно", &options(true, true)),
        CoreErrorCode::Locked,
    );
    assert_code(core.cancel_import("что-угодно"), CoreErrorCode::Locked);
}
