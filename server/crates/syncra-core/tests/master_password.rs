//! Смена мастер-пароля (F13) — то есть перешифровка хранилища.
//!
//! Главное обещание: секреты после смены читаются тем же способом и теми же
//! значениями, старый пароль больше ничего не открывает, а метаданные записей
//! не шелохнулись — смена ключа не является изменением записи (§5.2).

mod common;

use common::{assert_code, draft, MASTER_PASSWORD};
use syncra_core::{Core, CoreErrorCode, RecordDraft};

const NEW_MASTER_PASSWORD: &str = "мастер-пароль-2-подлиннее";

fn with_secrets() -> RecordDraft {
    let mut item = draft("Github", "octocat", "пароль-от-гитхаба");
    item.notes = Some("коды восстановления: 1234".to_owned());
    item.totp_secret = Some("JBSWY3DPEHPK3PXP".to_owned());
    item
}

#[test]
fn secrets_survive_the_change_and_the_old_password_stops_working() {
    let mut core = common::unlocked();
    let record = core.create_record(&with_secrets()).unwrap();
    let before = core.get_secret(&record.record_id).unwrap();

    core.change_master_password(MASTER_PASSWORD, NEW_MASTER_PASSWORD)
        .unwrap();

    // Хранилище остаётся открытым: пароль только что подтвердили.
    assert!(core.is_unlocked());
    assert_eq!(core.get_secret(&record.record_id).unwrap(), before);

    core.lock();
    assert_code(
        core.unlock(MASTER_PASSWORD),
        CoreErrorCode::InvalidMasterPassword,
    );
    core.unlock(NEW_MASTER_PASSWORD).unwrap();
    assert_eq!(core.get_secret(&record.record_id).unwrap(), before);
}

#[test]
fn rekeying_is_not_an_edit_of_the_records() {
    let mut core = common::unlocked();
    let before = core.create_record(&with_secrets()).unwrap();

    core.change_master_password(MASTER_PASSWORD, NEW_MASTER_PASSWORD)
        .unwrap();

    let after = core.list_records(None, false).unwrap().remove(0);
    // Ни версии, ни дат: иначе синхронизация приняла бы смену пароля на одном
    // устройстве за правку каждой записи (§5.2).
    assert_eq!(after, before);
}

#[test]
fn the_current_password_is_required_even_though_the_vault_is_open() {
    let mut core = common::unlocked();

    assert_code(
        core.change_master_password("не-тот-пароль", NEW_MASTER_PASSWORD),
        CoreErrorCode::InvalidMasterPassword,
    );

    // Неудачная попытка ничего не поменяла: старый пароль всё ещё открывает.
    core.lock();
    core.unlock(MASTER_PASSWORD).unwrap();
}

#[test]
fn the_new_password_has_to_be_a_password_and_a_different_one() {
    let mut core = common::unlocked();

    assert_code(
        core.change_master_password(MASTER_PASSWORD, "коротк"),
        CoreErrorCode::Validation,
    );
    assert_code(
        core.change_master_password(MASTER_PASSWORD, MASTER_PASSWORD),
        CoreErrorCode::Validation,
    );
}

#[test]
fn changing_the_password_needs_an_open_vault() {
    let mut core = common::unlocked();
    core.lock();

    assert_code(
        core.change_master_password(MASTER_PASSWORD, NEW_MASTER_PASSWORD),
        CoreErrorCode::Locked,
    );

    let mut fresh = Core::in_memory().unwrap();
    assert_code(
        fresh.change_master_password(MASTER_PASSWORD, NEW_MASTER_PASSWORD),
        CoreErrorCode::NotInitialized,
    );
}

#[test]
fn every_record_is_rekeyed_including_tombstones_neighbours() {
    let mut core = common::unlocked();

    let kept = core.create_record(&with_secrets()).unwrap();
    let doomed = core
        .create_record(&draft("Gitlab", "octocat", "пароль-2"))
        .unwrap();
    let empty = core
        .create_record(&draft("Figma", "designer", "пароль-3"))
        .unwrap();
    core.delete_record(&doomed.record_id).unwrap();

    core.change_master_password(MASTER_PASSWORD, NEW_MASTER_PASSWORD)
        .unwrap();
    core.lock();
    core.unlock(NEW_MASTER_PASSWORD).unwrap();

    // Надгробие секретов не хранит вовсе, а соседи открываются новым ключом.
    assert_eq!(
        core.get_secret(&kept.record_id).unwrap().password,
        "пароль-от-гитхаба"
    );
    assert_eq!(
        core.get_secret(&empty.record_id).unwrap().password,
        "пароль-3"
    );
    assert_code(core.get_secret(&doomed.record_id), CoreErrorCode::NotFound);
}

#[test]
fn settings_and_profile_are_untouched_by_the_change() {
    let mut core = common::unlocked();
    let profile = core.generator_profile().unwrap();
    let settings = core.security_settings().unwrap();

    core.change_master_password(MASTER_PASSWORD, NEW_MASTER_PASSWORD)
        .unwrap();

    assert_eq!(core.generator_profile().unwrap(), profile);
    assert_eq!(core.security_settings().unwrap(), settings);
}
