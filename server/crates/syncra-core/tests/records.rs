//! CRUD записей (F4, F5) — поведение сверено с `client/src/core/mock/index.ts`.

mod common;

use common::{assert_code, draft, unlocked};
use syncra_core::{CoreErrorCode, RecordPatch};

#[test]
fn created_record_starts_at_version_one() {
    let core = unlocked();
    let record = core
        .create_record(&draft("Google", "me@example.com", "пароль-1"))
        .unwrap();

    assert_eq!(record.version, 1);
    assert_eq!(record.deleted_at, None);
    // Все три даты у новой записи совпадают: менять ещё нечего.
    assert_eq!(record.created_at, record.updated_at);
    assert_eq!(record.created_at, record.password_updated_at);
    assert!(!record.has_notes);
    assert!(!record.has_totp);
    assert!(!record.record_id.is_empty());
}

#[test]
fn record_lands_in_default_vault_when_none_given() {
    let core = unlocked();
    let default_vault = core.list_vaults().unwrap().remove(0);

    let record = core
        .create_record(&draft("Google", "me@example.com", "пароль-1"))
        .unwrap();

    assert_eq!(record.vault_id, default_vault.vault_id);
}

#[test]
fn unknown_vault_is_refused_before_anything_is_written() {
    let core = unlocked();
    let mut item = draft("Google", "me@example.com", "пароль-1");
    item.vault_id = Some("нет-такой-секции".to_owned());

    assert_code(core.create_record(&item), CoreErrorCode::NotFound);
    assert!(core.list_records(None, false).unwrap().is_empty());
}

#[test]
fn required_fields_are_validated() {
    let core = unlocked();

    assert_code(
        core.create_record(&draft("   ", "me@example.com", "пароль")),
        CoreErrorCode::Validation,
    );
    assert_code(
        core.create_record(&draft("Google", "  ", "пароль")),
        CoreErrorCode::Validation,
    );
    assert_code(
        core.create_record(&draft("Google", "me@example.com", "   ")),
        CoreErrorCode::Validation,
    );
}

/// Запись, которая не влезает в кадр обмена, отвергается ПРИ СОЗДАНИИ.
///
/// Без потолка она спокойно ложилась в БД, а потом рвала круг обмена каждую
/// минуту до конца времён: `write_frame` отвергает кадр больше мегабайта, круг
/// повторяется, запись не уезжает никогда, и человеку никто не объясняет,
/// из-за чего (§5.4 — худшая из поломок).
#[test]
fn a_record_too_big_for_a_frame_is_refused_when_it_is_created() {
    let core = unlocked();

    let huge_secret = "я".repeat(syncra_core::model::SECRET_FIELD_MAX_BYTES);
    let huge_meta = "я".repeat(syncra_core::model::META_FIELD_MAX_BYTES);

    // Пароль, заметка и секрет TOTP — каждый под своим потолком.
    assert_code(
        core.create_record(&draft("Google", "me@example.com", &huge_secret)),
        CoreErrorCode::Validation,
    );
    let mut with_notes = draft("Google", "me@example.com", "пароль");
    with_notes.notes = Some(huge_secret.clone());
    assert_code(core.create_record(&with_notes), CoreErrorCode::Validation);

    let mut with_totp = draft("Google", "me@example.com", "пароль");
    with_totp.totp_secret = Some(huge_secret);
    assert_code(core.create_record(&with_totp), CoreErrorCode::Validation);

    // Метаданные — своим, он на два порядка ниже.
    assert_code(
        core.create_record(&draft(&huge_meta, "me@example.com", "пароль")),
        CoreErrorCode::Validation,
    );
    assert_code(
        core.create_record(&draft("Google", &huge_meta, "пароль")),
        CoreErrorCode::Validation,
    );
    let mut with_label = draft("Google", "me@example.com", "пароль");
    with_label.account_label = Some(huge_meta.clone());
    assert_code(core.create_record(&with_label), CoreErrorCode::Validation);

    // Адреса: и длина каждого, и их число.
    let mut with_urls = draft("Google", "me@example.com", "пароль");
    with_urls.urls = vec![huge_meta];
    assert_code(core.create_record(&with_urls), CoreErrorCode::Validation);

    let mut many_urls = draft("Google", "me@example.com", "пароль");
    many_urls.urls = (0..=syncra_core::model::RECORD_URLS_MAX)
        .map(|index| format!("site{index}.com"))
        .collect();
    assert_code(core.create_record(&many_urls), CoreErrorCode::Validation);

    // Ни одна из них не легла в хранилище.
    assert!(core.list_records(None, false).unwrap().is_empty());
}

/// Тот же потолок — и на правке: иначе запись переезжала бы через край
/// вторым действием.
#[test]
fn the_same_ceilings_hold_when_a_record_is_edited() {
    let core = unlocked();
    let created = core
        .create_record(&draft("Google", "me@example.com", "пароль"))
        .unwrap();

    let huge_secret = "я".repeat(syncra_core::model::SECRET_FIELD_MAX_BYTES);
    assert_code(
        core.update_record(
            &created.record_id,
            &RecordPatch {
                notes: Some(Some(huge_secret)),
                ..RecordPatch::default()
            },
        ),
        CoreErrorCode::Validation,
    );
    assert_code(
        core.update_record(
            &created.record_id,
            &RecordPatch {
                service_name: Some("я".repeat(syncra_core::model::META_FIELD_MAX_BYTES)),
                ..RecordPatch::default()
            },
        ),
        CoreErrorCode::Validation,
    );

    // Запись цела и версия не двинулась: отказ случился до записи.
    let after = core.list_records(None, false).unwrap();
    assert_eq!(after[0].version, created.version);
    assert!(!after[0].has_notes);
}

/// Все потолки сразу — и всё равно втрое меньше кадра.
///
/// Арифметика, а не поведение: она держит потолки согласованными с
/// `net::wire::MAX_FRAME`, если кто-то поднимет один из них не глядя.
#[test]
fn all_the_ceilings_together_still_fit_into_a_frame() {
    let secrets = 3 * syncra_core::model::SECRET_FIELD_MAX_BYTES;
    let meta = 3 * syncra_core::model::META_FIELD_MAX_BYTES
        + syncra_core::model::RECORD_URLS_MAX * syncra_core::model::META_FIELD_MAX_BYTES;

    // Половина кадра — запас на кодирование, шифрование и метаданные секции.
    assert!(
        secrets + meta < syncra_core::net::wire::MAX_FRAME / 2,
        "запись на всех потолках сразу перестала влезать в кадр"
    );
}

#[test]
fn metadata_is_trimmed_but_password_is_kept_byte_for_byte() {
    let core = unlocked();
    // Пробел по краям пароля — легитимный символ, и обрезать его нельзя.
    let record = core
        .create_record(&draft("  Google  ", "  me@example.com  ", "  пароль  "))
        .unwrap();

    assert_eq!(record.service_name, "Google");
    assert_eq!(record.login, "me@example.com");
    assert_eq!(
        core.get_secret(&record.record_id).unwrap().password,
        "  пароль  "
    );
}

#[test]
fn secrets_round_trip() {
    let core = unlocked();
    let mut item = draft("GitHub", "octocat", "пароль-2");
    item.notes = Some("код восстановления 1234".to_owned());
    item.totp_secret = Some("JBSWY3DPEHPK3PXP".to_owned());

    let record = core.create_record(&item).unwrap();
    assert!(record.has_notes);
    assert!(record.has_totp);

    let secrets = core.get_secret(&record.record_id).unwrap();
    assert_eq!(secrets.password, "пароль-2");
    assert_eq!(secrets.notes.as_deref(), Some("код восстановления 1234"));
    assert_eq!(secrets.totp_secret.as_deref(), Some("JBSWY3DPEHPK3PXP"));
}

#[test]
fn blank_note_means_there_is_no_note() {
    let core = unlocked();
    let mut item = draft("Steam", "gamer", "пароль-3");
    item.notes = Some("   ".to_owned());

    let record = core.create_record(&item).unwrap();
    assert!(!record.has_notes);
    assert_eq!(core.get_secret(&record.record_id).unwrap().notes, None);
}

#[test]
fn update_bumps_version_but_not_password_date() {
    let core = unlocked();
    let created = core
        .create_record(&draft("Google", "me@example.com", "пароль-1"))
        .unwrap();

    let updated = core
        .update_record(
            &created.record_id,
            &RecordPatch {
                service_name: Some("Google Workspace".to_owned()),
                ..RecordPatch::default()
            },
        )
        .unwrap();

    assert_eq!(updated.version, 2);
    assert_eq!(updated.service_name, "Google Workspace");
    assert_eq!(updated.password_updated_at, created.password_updated_at);
    assert_eq!(updated.created_at, created.created_at);
}

#[test]
fn resending_the_same_password_does_not_count_as_a_change() {
    // Форма записи присылает пароль и тогда, когда человек его не трогал.
    // `password_updated_at` — про смену значения, а не про приход поля.
    let core = unlocked();
    let created = core
        .create_record(&draft("Google", "me@example.com", "пароль-1"))
        .unwrap();

    let updated = core
        .update_record(
            &created.record_id,
            &RecordPatch {
                password: Some("пароль-1".to_owned()),
                ..RecordPatch::default()
            },
        )
        .unwrap();

    assert_eq!(updated.version, 2);
    assert_eq!(updated.password_updated_at, created.password_updated_at);
}

#[test]
fn changing_the_password_moves_its_own_date() {
    let core = unlocked();
    let created = core
        .create_record(&draft("Google", "me@example.com", "пароль-1"))
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(5));
    let updated = core
        .update_record(
            &created.record_id,
            &RecordPatch {
                password: Some("пароль-2".to_owned()),
                ..RecordPatch::default()
            },
        )
        .unwrap();

    assert_ne!(updated.password_updated_at, created.password_updated_at);
    assert_eq!(updated.password_updated_at, updated.updated_at);
    assert_eq!(
        core.get_secret(&created.record_id).unwrap().password,
        "пароль-2"
    );
}

#[test]
fn absent_patch_field_means_do_not_touch_and_null_means_clear() {
    let core = unlocked();
    let mut item = draft("GitHub", "octocat", "пароль-2");
    item.notes = Some("важное".to_owned());
    item.account_label = Some("рабочий".to_owned());
    let created = core.create_record(&item).unwrap();

    // Патч без заметки — заметка на месте.
    let untouched = core
        .update_record(
            &created.record_id,
            &RecordPatch {
                login: Some("octocat-2".to_owned()),
                ..RecordPatch::default()
            },
        )
        .unwrap();
    assert!(untouched.has_notes);
    assert_eq!(untouched.account_label.as_deref(), Some("рабочий"));

    // Явный `null` — стереть.
    let cleared = core
        .update_record(
            &created.record_id,
            &RecordPatch {
                notes: Some(None),
                account_label: Some(None),
                ..RecordPatch::default()
            },
        )
        .unwrap();
    assert!(!cleared.has_notes);
    assert_eq!(cleared.account_label, None);
    assert_eq!(core.get_secret(&created.record_id).unwrap().notes, None);
}

#[test]
fn delete_leaves_a_tombstone_without_secrets() {
    let core = unlocked();
    let mut item = draft("Jira", "worker", "пароль-4");
    item.notes = Some("заметка".to_owned());
    item.totp_secret = Some("JBSWY3DPEHPK3PXP".to_owned());
    let created = core.create_record(&item).unwrap();

    let tombstone = core.delete_record(&created.record_id).unwrap();

    assert_eq!(tombstone.record_id, created.record_id);
    assert_eq!(tombstone.version, created.version + 1);
    assert!(tombstone.deleted_at.is_some());
    // Надгробие не хранит секретов — значит, и «есть заметка» больше не правда.
    assert!(!tombstone.has_notes);
    assert!(!tombstone.has_totp);

    // Секретов у него больше нет ни в каком виде.
    assert_code(core.get_secret(&created.record_id), CoreErrorCode::NotFound);
}

#[test]
fn tombstones_are_hidden_unless_asked_for() {
    let core = unlocked();
    let alive = core
        .create_record(&draft("Google", "me@example.com", "пароль-1"))
        .unwrap();
    let dead = core
        .create_record(&draft("Jira", "worker", "пароль-4"))
        .unwrap();
    core.delete_record(&dead.record_id).unwrap();

    let visible = core.list_records(None, false).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].record_id, alive.record_id);

    assert_eq!(core.list_records(None, true).unwrap().len(), 2);
}

#[test]
fn deleted_record_cannot_be_edited_or_deleted_again() {
    let core = unlocked();
    let created = core
        .create_record(&draft("Jira", "worker", "пароль-4"))
        .unwrap();
    core.delete_record(&created.record_id).unwrap();

    assert_code(
        core.update_record(&created.record_id, &RecordPatch::default()),
        CoreErrorCode::NotFound,
    );
    assert_code(
        core.delete_record(&created.record_id),
        CoreErrorCode::NotFound,
    );
}

#[test]
fn unknown_record_is_not_found() {
    let core = unlocked();
    assert_code(core.get_secret("нет-такой"), CoreErrorCode::NotFound);
    assert_code(
        core.update_record("нет-такой", &RecordPatch::default()),
        CoreErrorCode::NotFound,
    );
    assert_code(core.delete_record("нет-такой"), CoreErrorCode::NotFound);
}

#[test]
fn list_filters_by_vault_and_sorts_by_service_then_login() {
    let core = unlocked();
    let work = core.create_vault("Рабочее", "amber").unwrap();

    core.create_record(&draft("Steam", "gamer", "пароль"))
        .unwrap();
    core.create_record(&draft("google", "b@example.com", "пароль"))
        .unwrap();
    core.create_record(&draft("Google", "a@example.com", "пароль"))
        .unwrap();

    let mut in_work = draft("Jira", "worker", "пароль");
    in_work.vault_id = Some(work.vault_id.clone());
    core.create_record(&in_work).unwrap();

    let all = core.list_records(None, false).unwrap();
    let order: Vec<_> = all
        .iter()
        .map(|record| (record.service_name.as_str(), record.login.as_str()))
        .collect();
    assert_eq!(
        order,
        vec![
            ("Google", "a@example.com"),
            ("google", "b@example.com"),
            ("Jira", "worker"),
            ("Steam", "gamer"),
        ]
    );

    let only_work = core.list_records(Some(&work.vault_id), false).unwrap();
    assert_eq!(only_work.len(), 1);
    assert_eq!(only_work[0].service_name, "Jira");
}

#[test]
fn record_can_be_moved_between_vaults() {
    let core = unlocked();
    let work = core.create_vault("Рабочее", "amber").unwrap();
    let created = core
        .create_record(&draft("Jira", "worker", "пароль-4"))
        .unwrap();

    let moved = core
        .update_record(
            &created.record_id,
            &RecordPatch {
                vault_id: Some(work.vault_id.clone()),
                ..RecordPatch::default()
            },
        )
        .unwrap();

    assert_eq!(moved.vault_id, work.vault_id);
    assert_eq!(moved.version, 2);

    // Чужая секция в патче не должна ни переместить запись, ни сломать её.
    assert_code(
        core.update_record(
            &created.record_id,
            &RecordPatch {
                vault_id: Some("нет-такой-секции".to_owned()),
                ..RecordPatch::default()
            },
        ),
        CoreErrorCode::NotFound,
    );
    let after = core.list_records(None, false).unwrap();
    assert_eq!(after[0].vault_id, work.vault_id);
    assert_eq!(after[0].version, 2);
}
