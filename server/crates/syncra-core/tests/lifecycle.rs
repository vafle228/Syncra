//! Жизненный цикл хранилища (F3).
//!
//! Зеркалит `client/src/core/__tests__/vault-lifecycle.spec.ts`.

mod common;

use common::{assert_code, draft, unlocked, MASTER_PASSWORD};
use syncra_core::{Core, CoreErrorCode};

#[test]
fn fresh_device_has_no_vault() {
    let core = Core::in_memory().unwrap();
    let status = core.status().unwrap();

    assert!(!status.initialized);
    assert!(!status.unlocked);
    assert_eq!(status.unlocked_at, None);
    // Быстрый вход в этом шаге не заводится — но поле должно приезжать всегда.
    assert!(!status.pin.enrolled);
    assert_eq!(status.pin.attempts_left, None);
}

#[test]
fn init_creates_vault_and_leaves_it_open() {
    let mut core = Core::in_memory().unwrap();
    let response = core.init_vault(MASTER_PASSWORD).unwrap();

    let status = core.status().unwrap();
    assert!(status.initialized);
    assert!(status.unlocked);
    assert_eq!(status.unlocked_at.as_deref(), Some(&*response.unlocked_at));

    // Одна секция по умолчанию — и ни одной записи: пароли заводит пользователь.
    let vaults = core.list_vaults().unwrap();
    assert_eq!(vaults.len(), 1);
    assert!(vaults[0].is_default);
    assert!(vaults[0].sync);
    assert!(core.list_records(None, false).unwrap().is_empty());
}

#[test]
fn init_on_existing_vault_is_refused() {
    let mut core = unlocked();
    assert_code(
        core.init_vault("другой-пароль"),
        CoreErrorCode::AlreadyInitialized,
    );
}

#[test]
fn short_master_password_is_refused() {
    let mut core = Core::in_memory().unwrap();
    assert_code(core.init_vault("1234567"), CoreErrorCode::Validation);
    // Отказ не должен был оставить полусозданное хранилище.
    assert!(!core.status().unwrap().initialized);
}

#[test]
fn lock_closes_the_vault() {
    let mut core = unlocked();
    core.lock();

    let status = core.status().unwrap();
    assert!(status.initialized);
    assert!(!status.unlocked);
    assert_eq!(status.unlocked_at, None);
}

#[test]
fn locked_vault_answers_locked_to_everything_else() {
    let mut core = unlocked();
    core.lock();

    assert_code(core.list_records(None, false), CoreErrorCode::Locked);
    assert_code(core.list_vaults(), CoreErrorCode::Locked);
    assert_code(core.get_secret("что-угодно"), CoreErrorCode::Locked);
    assert_code(
        core.create_record(&draft("Google", "me@example.com", "пароль")),
        CoreErrorCode::Locked,
    );
    assert_code(core.create_vault("Рабочее", "amber"), CoreErrorCode::Locked);
}

#[test]
fn uninitialized_vault_answers_not_initialized_not_locked() {
    // Разница принципиальная: по `LOCKED` UI ушёл бы на экран входа, а нужно —
    // на онбординг.
    let core = Core::in_memory().unwrap();

    assert_code(
        core.list_records(None, false),
        CoreErrorCode::NotInitialized,
    );
    assert_code(core.list_vaults(), CoreErrorCode::NotInitialized);
}

#[test]
fn unlock_requires_the_right_master_password() {
    let mut core = unlocked();
    core.lock();

    assert_code(
        core.unlock("не-тот-пароль"),
        CoreErrorCode::InvalidMasterPassword,
    );
    assert!(!core.is_unlocked());

    core.unlock(MASTER_PASSWORD).unwrap();
    assert!(core.is_unlocked());
    assert!(core.list_vaults().is_ok());
}

#[test]
fn unlock_on_uninitialized_vault_is_refused() {
    let mut core = Core::in_memory().unwrap();
    assert_code(core.unlock(MASTER_PASSWORD), CoreErrorCode::NotInitialized);
}

#[test]
fn vault_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let record_id = {
        let mut core = Core::open(&path).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();
        core.create_record(&draft("Google", "me@example.com", "пароль-1"))
            .unwrap()
            .record_id
    };

    // Новый процесс: то же хранилище, заперто, открывается тем же паролем.
    let mut core = Core::open(&path).unwrap();
    let status = core.status().unwrap();
    assert!(status.initialized);
    assert!(!status.unlocked);

    core.unlock(MASTER_PASSWORD).unwrap();
    let records = core.list_records(None, false).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, record_id);
    assert_eq!(core.get_secret(&record_id).unwrap().password, "пароль-1");
}
