//! Секции (F7, §4.2) — зеркалит `client/src/core/__tests__/vaults.spec.ts`.

mod common;

use common::{assert_code, draft, unlocked};
use syncra_core::{CoreErrorCode, VaultPatch};

#[test]
fn new_vault_syncs_and_is_not_default() {
    let core = unlocked();
    let vault = core.create_vault("Рабочее", "amber").unwrap();

    // Продукт про синхронизацию: молча оставлять записи на одном устройстве было
    // бы сюрпризом. Выключается тумблером, осознанно.
    assert!(vault.sync);
    assert!(!vault.is_default);
    assert_eq!(vault.name, "Рабочее");
    assert_eq!(vault.color, "amber");
}

#[test]
fn vault_order_follows_creation_not_name() {
    let core = unlocked();
    core.create_vault("Яблоки", "amber").unwrap();
    core.create_vault("Апельсины", "mint").unwrap();

    let names: Vec<_> = core
        .list_vaults()
        .unwrap()
        .into_iter()
        .map(|vault| vault.name)
        .collect();
    // Сайдбар не должен перетасовываться от переименования соседа.
    assert_eq!(names, vec!["Личное", "Яблоки", "Апельсины"]);
}

#[test]
fn vault_name_and_color_are_validated() {
    let core = unlocked();

    assert_code(core.create_vault("   ", "amber"), CoreErrorCode::Validation);
    assert_code(
        core.create_vault(&"я".repeat(41), "amber"),
        CoreErrorCode::Validation,
    );
    assert_code(
        core.create_vault("Рабочее", "oklch(70% 0.2 250)"),
        CoreErrorCode::Validation,
    );

    // Ровно 40 символов — ещё можно.
    assert!(core.create_vault(&"я".repeat(40), "coral").is_ok());
}

#[test]
fn update_changes_only_name_and_color() {
    let core = unlocked();
    let vault = core.create_vault("Рабочее", "amber").unwrap();

    let renamed = core
        .update_vault(
            &vault.vault_id,
            &VaultPatch {
                name: Some("  Работа  ".to_owned()),
                color: None,
            },
        )
        .unwrap();

    assert_eq!(renamed.name, "Работа");
    assert_eq!(renamed.color, "amber");
    assert_eq!(renamed.sync, vault.sync);
    assert_eq!(renamed.created_at, vault.created_at);
}

#[test]
fn sync_flag_is_its_own_command() {
    let core = unlocked();
    let vault = core.create_vault("Рабочее", "amber").unwrap();

    let local_only = core.set_vault_sync(&vault.vault_id, false).unwrap();
    assert!(!local_only.sync);
    assert!(core.set_vault_sync(&vault.vault_id, true).unwrap().sync);
}

#[test]
fn exactly_one_vault_is_default() {
    let mut core = unlocked();
    let personal = core.list_vaults().unwrap().remove(0);
    let work = core.create_vault("Рабочее", "amber").unwrap();

    let vaults = core.set_default_vault(&work.vault_id).unwrap();

    assert_eq!(vaults.len(), 2);
    let defaults: Vec<_> = vaults.iter().filter(|vault| vault.is_default).collect();
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0].vault_id, work.vault_id);
    assert!(
        !vaults
            .iter()
            .find(|vault| vault.vault_id == personal.vault_id)
            .unwrap()
            .is_default
    );
}

#[test]
fn default_vault_cannot_be_deleted() {
    let mut core = unlocked();
    let personal = core.list_vaults().unwrap().remove(0);

    assert_code(
        core.delete_vault(&personal.vault_id),
        CoreErrorCode::Validation,
    );
    assert_eq!(core.list_vaults().unwrap().len(), 1);
}

#[test]
fn deleting_a_vault_moves_its_records_instead_of_erasing_them() {
    let mut core = unlocked();
    let personal = core.list_vaults().unwrap().remove(0);
    let work = core.create_vault("Рабочее", "amber").unwrap();

    let mut item = draft("Jira", "worker", "пароль-4");
    item.vault_id = Some(work.vault_id.clone());
    let created = core.create_record(&item).unwrap();

    // Даты в ядре — с точностью до миллисекунды, а создание и удаление здесь
    // укладываются в одну. Без паузы проверка «дата обновилась» превращается в
    // подбрасывание монетки.
    std::thread::sleep(std::time::Duration::from_millis(5));
    let remaining = core.delete_vault(&work.vault_id).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].vault_id, personal.vault_id);

    let records = core.list_records(None, false).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].vault_id, personal.vault_id);
    // Переезд — обычная правка записи: на другом устройстве он должен быть виден.
    assert_eq!(records[0].version, created.version + 1);
    assert_ne!(records[0].updated_at, created.updated_at);

    // Секрет переезд не задел.
    assert_eq!(
        core.get_secret(&created.record_id).unwrap().password,
        "пароль-4"
    );
}

#[test]
fn unknown_vault_is_not_found() {
    let mut core = unlocked();

    assert_code(
        core.update_vault("нет-такой", &VaultPatch::default()),
        CoreErrorCode::NotFound,
    );
    assert_code(
        core.set_vault_sync("нет-такой", false),
        CoreErrorCode::NotFound,
    );
    assert_code(core.set_default_vault("нет-такой"), CoreErrorCode::NotFound);
    assert_code(core.delete_vault("нет-такой"), CoreErrorCode::NotFound);
}
