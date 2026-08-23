//! Форма ответов на проводе — договор с фронтом (`client/src/core/contract.ts`).
//!
//! Типы ядра сериализуются прямо в IPC-ответы, поэтому переименованное поле — это
//! не рефакторинг, а сломанный экран. Здесь ответы осматриваются ровно так, как
//! их увидит фронт: как JSON.

mod common;

use serde_json::{json, Value};
use syncra_core::{
    ConflictField, ConflictSecrets, ConflictSide, ConflictVersion, Core, DeviceKind,
    GeneratorProfile, ImportOptions, ImportRowStatus, ImportSource, PeerFound, RecordConflict,
    SecuritySettings, SyncPhase, SyncStatus,
};

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("сериализация")
}

#[test]
fn generator_profile_looks_like_the_contract() {
    let core = common::unlocked();

    assert_eq!(
        json_of(&core.generator_profile().unwrap()),
        json!({
            "mode": "chars",
            "length": 20,
            "digits": true,
            "symbols": true,
            "avoid_ambiguous": true,
            "words": 4,
            "separator": "-",
            "append_number": false,
        })
    );
}

#[test]
fn generated_passwords_look_like_the_contract() {
    let core = common::unlocked();
    let response = json_of(&core.generate_passwords(2, None).unwrap());

    assert_eq!(response["passwords"].as_array().unwrap().len(), 2);
    assert!(response["entropy_bits"].is_i64());
    // Ни правил, ни профиля в ответе: фронт получает строки и оценку, и только.
    assert_eq!(response.as_object().unwrap().len(), 2);
}

#[test]
fn security_settings_look_like_the_contract() {
    let core = common::unlocked();

    assert_eq!(
        json_of(&core.security_settings().unwrap()),
        json!({
            "autolock_ms": 300_000,
            "clipboard_clear_ms": 20_000,
            "secret_reveal_ms": 30_000,
        })
    );
    // Умолчания ядра — те же числа, что и `DEFAULT_SECURITY_SETTINGS` во фронте.
    assert_eq!(
        json_of(&SecuritySettings::default()),
        json_of(&core.security_settings().unwrap())
    );
}

#[test]
fn change_master_password_looks_like_the_contract() {
    let mut core = common::unlocked();
    let response = json_of(
        &core
            .change_master_password(common::MASTER_PASSWORD, "мастер-пароль-2-подлиннее")
            .unwrap(),
    );

    assert!(response["changed_at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(response["devices_to_update"], json!(0));
    assert_eq!(response.as_object().unwrap().len(), 2);
}

#[test]
fn a_profile_from_the_wire_is_accepted_as_is() {
    let core = common::unlocked();

    // Ровно то, что пришлёт `saveGeneratorProfile` (`ipc.ts:341`).
    let from_ui: GeneratorProfile = serde_json::from_value(json!({
        "mode": "words",
        "length": 20,
        "digits": true,
        "symbols": false,
        "avoid_ambiguous": true,
        "words": 6,
        "separator": " ",
        "append_number": true,
    }))
    .expect("разбор профиля");

    let saved = core.save_generator_profile(&from_ui).unwrap();
    assert_eq!(saved, from_ui);
    assert_eq!(json_of(&saved)["separator"], json!(" "));
}

#[test]
fn a_device_looks_like_the_contract() {
    let core = common::unlocked();
    let devices = json_of(&core.list_devices().unwrap());
    let device = devices[0].as_object().expect("объект устройства");

    // ЗАКОН №1 в форме ответа: ключей и сетевых адресов в списке нет (§2.1).
    // Тот же набор сверяет фронт — `core/__tests__/devices.spec.ts`.
    let mut keys: Vec<&str> = device.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "device_id",
            "fingerprint_words",
            "is_this_device",
            "kind",
            "last_seen_at",
            "name",
            "paired_at",
            "revoked_at",
        ]
    );

    assert_eq!(device["kind"], json!("desktop"));
    assert_eq!(device["is_this_device"], json!(true));
    assert_eq!(device["revoked_at"], Value::Null);
    assert_eq!(device["fingerprint_words"].as_array().unwrap().len(), 4);
}

#[test]
fn a_pairing_offer_looks_like_the_contract() {
    let mut core = common::unlocked();
    let offer = json_of(&core.get_pairing_payload().unwrap());
    let offer = offer.as_object().expect("объект кода");

    let mut keys: Vec<&str> = offer.keys().map(String::as_str).collect();
    keys.sort_unstable();
    // Тот же набор сверяет фронт — `core/__tests__/pairing.spec.ts`. Строки
    // пейлоада среди них нет: в нём ключи, и наружу уходит только картинка.
    assert_eq!(keys, ["expires_at", "manual_code", "qr", "session_id"]);

    let size = offer["qr"]["size"].as_u64().expect("сторона матрицы");
    let rows = offer["qr"]["rows"].as_array().expect("строки матрицы");
    assert_eq!(rows.len() as u64, size);
    assert!(rows
        .iter()
        .all(|row| row.as_str().expect("строка").len() as u64 == size));
}

#[test]
fn a_pairing_handshake_and_its_result_look_like_the_contract() {
    let mut core = common::unlocked();
    let mut peer = syncra_core::Core::in_memory(syncra_core::HostDevice::new(
        "Телефон",
        syncra_core::DeviceKind::Mobile,
    ))
    .unwrap();
    peer.init_vault("мастер-пароль-2").unwrap();
    peer.get_pairing_payload().unwrap();
    let payload = peer.shown_pairing_payload().unwrap().to_owned();

    let handshake = core.submit_paired_key(&payload).unwrap();
    let wire = json_of(&handshake);
    let mut keys: Vec<&str> = wire
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "expires_at",
            "fingerprint_words",
            "peer_kind",
            "peer_name",
            "session_id",
        ]
    );
    assert_eq!(wire["peer_kind"], json!("mobile"));
    assert_eq!(wire["fingerprint_words"].as_array().unwrap().len(), 4);

    let result = json_of(&core.confirm_pairing(&handshake.session_id).unwrap());
    let mut keys: Vec<&str> = result
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["device", "duration_ms", "records_transferred"]);
    assert_eq!(result["device"]["is_this_device"], json!(false));
    assert_eq!(result["records_transferred"], json!(0));
}

#[test]
fn a_sync_status_looks_like_the_contract() {
    // `SyncStatus` фронт не склеивает по кусочкам: событие `sync_status` несёт
    // ПОЛНЫЙ статус, и `useSyncStore` заменяет им состояние целиком
    // (`contract.ts:1290`). Значит, набор ключей — часть договора.
    let wire = json_of(&SyncStatus::idle());

    let mut keys: Vec<&str> = wire
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "last_sync_at",
            "message",
            "peer_name",
            "peers_online",
            "pending_records",
            "phase",
        ]
    );

    // Счётчика конфликтов здесь намеренно нет: он приходит из `list_conflicts`,
    // и две правды об одном числе рано или поздно разъедутся.
    assert!(wire.get("conflicts").is_none());

    assert_eq!(wire["phase"], json!("idle"));
    assert_eq!(wire["peers_online"], json!(0));
    assert_eq!(wire["peer_name"], json!(null));
    assert_eq!(wire["pending_records"], json!([]));
    assert_eq!(wire["last_sync_at"], json!(null));
    assert_eq!(wire["message"], json!(null));
}

fn conflict_version(side: ConflictSide) -> ConflictVersion {
    ConflictVersion {
        side,
        device_name: "Телефон".to_owned(),
        version: 7,
        updated_at: "2026-03-02T08:12:40.000Z".to_owned(),
        vault_id: "v1".to_owned(),
        service_name: "GitHub".to_owned(),
        urls: vec!["github.com".to_owned()],
        login: "demo-user".to_owned(),
        account_label: None,
        has_notes: true,
        has_totp: false,
    }
}

#[test]
fn a_record_conflict_looks_like_the_contract() {
    // Тот же конфликт приезжает и списком, и событием `conflict_raised`
    // (`contract.ts:1324`), поэтому набор ключей — часть договора, а не деталь.
    let wire = json_of(&RecordConflict {
        record_id: "r1".to_owned(),
        raised_at: "2026-03-02T08:13:05.000Z".to_owned(),
        local: conflict_version(ConflictSide::Local),
        remote: conflict_version(ConflictSide::Remote),
        differing_fields: vec![ConflictField::Urls, ConflictField::Password],
    });

    let mut keys: Vec<&str> = wire
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "differing_fields",
            "local",
            "raised_at",
            "record_id",
            "remote"
        ]
    );

    let mut side_keys: Vec<&str> = wire["local"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    side_keys.sort_unstable();
    assert_eq!(
        side_keys,
        [
            "account_label",
            "device_name",
            "has_notes",
            "has_totp",
            "login",
            "service_name",
            "side",
            "updated_at",
            "urls",
            "vault_id",
            "version",
        ]
    );

    assert_eq!(wire["local"]["side"], json!("local"));
    assert_eq!(wire["remote"]["side"], json!("remote"));
    // Поля перечислены ИМЕНАМИ и в том написании, которое знает `ConflictField`.
    assert_eq!(wire["differing_fields"], json!(["urls", "password"]));

    // ЗАКОН №1: ни одного секретного ЗНАЧЕНИЯ в конфликте нет — только флаги.
    assert!(wire["local"].get("password").is_none());
    assert!(wire["local"].get("notes").is_none());
    assert_eq!(wire["local"]["has_notes"], json!(true));
}

#[test]
fn every_conflict_field_is_spelled_the_way_the_contract_spells_it() {
    for (field, name) in [
        (ConflictField::VaultId, "vault_id"),
        (ConflictField::ServiceName, "service_name"),
        (ConflictField::Urls, "urls"),
        (ConflictField::Login, "login"),
        (ConflictField::AccountLabel, "account_label"),
        (ConflictField::Password, "password"),
        (ConflictField::Notes, "notes"),
        (ConflictField::TotpSecret, "totp_secret"),
    ] {
        assert_eq!(json_of(&field), json!(name));
    }
}

#[test]
fn a_conflict_secret_carries_both_sides_and_nothing_else() {
    let wire = json_of(&ConflictSecrets {
        local: Some("тайна-1".to_owned()),
        remote: None,
    });

    assert_eq!(wire, json!({ "local": "тайна-1", "remote": null }));
}

#[test]
fn every_sync_phase_is_spelled_the_way_the_contract_spells_it() {
    for (phase, name) in [
        (SyncPhase::Idle, "idle"),
        (SyncPhase::Searching, "searching"),
        (SyncPhase::Syncing, "syncing"),
        (SyncPhase::Error, "error"),
    ] {
        let status = SyncStatus {
            phase,
            ..SyncStatus::idle()
        };
        assert_eq!(json_of(&status)["phase"], json!(name));
    }
}

#[test]
fn a_found_peer_looks_like_the_contract() {
    let wire = json_of(&PeerFound {
        device_id: "device-2".to_owned(),
        name: "Телефон".to_owned(),
        kind: DeviceKind::Mobile,
        found_at: "2026-08-05T21:14:03.000Z".to_owned(),
    });

    let mut keys: Vec<&str> = wire
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["device_id", "found_at", "kind", "name"]);
    assert_eq!(wire["kind"], json!("mobile"));
}

// ---------------------------------------------------------------------------
// Перенос данных (F12, §6.2)
// ---------------------------------------------------------------------------

/// Ключи объекта по алфавиту — «полей ровно столько и ровно эти».
fn keys_of(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("объект").keys().cloned().collect();
    keys.sort_unstable();
    keys
}

#[test]
fn an_export_file_looks_like_the_contract() {
    let dir = tempfile::tempdir().unwrap();
    let core = common::unlocked();
    core.create_record(&common::draft("GitHub", "octocat", "тайна"))
        .unwrap();
    let mut core = core;

    let wire = json_of(
        &core
            .export_csv(common::MASTER_PASSWORD, dir.path())
            .unwrap(),
    );

    // Содержимого файла здесь нет и быть не может — только его след.
    assert_eq!(
        keys_of(&wire),
        [
            "created_at",
            "encrypted",
            "file_name",
            "path",
            "record_count",
            "size_bytes",
        ]
    );
    assert_eq!(wire["encrypted"], json!(false));
    assert_eq!(wire["record_count"], json!(1));
}

#[test]
fn an_import_preview_looks_like_the_contract() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("passwords.csv");
    std::fs::write(
        &file,
        "name,url,username,password\nFigma,figma.com,me,тайна\n",
    )
    .unwrap();

    let mut core = common::unlocked();
    let wire = json_of(&core.begin_import(ImportSource::Csv, &file).unwrap());

    assert_eq!(
        keys_of(&wire),
        [
            "duplicate_count",
            "file_name",
            "new_count",
            "no_password_count",
            "rows",
            "session_id",
            "source",
            "target_vault_name",
            "total_rows",
        ]
    );
    assert_eq!(wire["source"], json!("csv"));

    // ЗАКОН №1 в форме ответа: у строки предпросмотра нет поля под пароль.
    let row = &wire["rows"][0];
    assert_eq!(keys_of(row), ["login", "site", "status"]);
    assert_eq!(row["status"], json!("new"));
}

#[test]
fn an_import_result_looks_like_the_contract() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("passwords.csv");
    std::fs::write(
        &file,
        "name,url,username,password\nFigma,figma.com,me,тайна\n",
    )
    .unwrap();

    let mut core = common::unlocked();
    let preview = core.begin_import(ImportSource::Csv, &file).unwrap();
    let wire = json_of(
        &core
            .commit_import(
                &preview.session_id,
                &ImportOptions {
                    skip_duplicates: true,
                    flag_reused: true,
                },
            )
            .unwrap(),
    );

    assert_eq!(
        keys_of(&wire),
        [
            "imported",
            "reused_passwords",
            "skipped",
            "source_file_deleted",
            "vault",
        ]
    );
    // Секция приезжает целиком — фронту незачем перечитывать список ради неё.
    assert_eq!(
        keys_of(&wire["vault"]),
        [
            "color",
            "created_at",
            "is_default",
            "name",
            "sync",
            "vault_id"
        ]
    );
}

#[test]
fn a_restore_result_looks_like_the_contract() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = common::unlocked();
    let file = source
        .export_backup(common::MASTER_PASSWORD, dir.path())
        .unwrap();

    let fresh = tempfile::tempdir().unwrap();
    let mut restored = Core::open(&fresh.path().join("syncra.db"), common::host()).unwrap();
    let wire = json_of(
        &restored
            .restore_backup(common::MASTER_PASSWORD, std::path::Path::new(&file.path))
            .unwrap(),
    );

    assert_eq!(
        keys_of(&wire),
        [
            "file_name",
            "initialized_at",
            "records",
            "unlocked_at",
            "vaults",
        ]
    );
}

#[test]
fn every_import_row_status_is_spelled_the_way_the_contract_spells_it() {
    for (status, name) in [
        (ImportRowStatus::New, "new"),
        (ImportRowStatus::Duplicate, "duplicate"),
        (ImportRowStatus::NoPassword, "no_password"),
    ] {
        assert_eq!(json_of(&status), json!(name));
    }
}

#[test]
fn every_import_source_is_spelled_the_way_the_contract_spells_it() {
    // Оба направления: имя уходит в предпросмотр и приходит из запроса.
    for (source, name) in [
        (ImportSource::Chrome, "chrome"),
        (ImportSource::Firefox, "firefox"),
        (ImportSource::OnePassword, "1password"),
        (ImportSource::Bitwarden, "bitwarden"),
        (ImportSource::KeePass, "keepass"),
        (ImportSource::Csv, "csv"),
    ] {
        assert_eq!(json_of(&source), json!(name));
        assert_eq!(ImportSource::parse(name).unwrap(), source);
    }

    // Незнакомый источник — VALIDATION с человеческим текстом, а не провал
    // serde на границе Tauri (`mock/index.ts:1489`).
    assert_eq!(
        ImportSource::parse("lastpass").unwrap_err().code,
        syncra_core::CoreErrorCode::Validation
    );
}
