//! Форма ответов на проводе — договор с фронтом (`client/src/core/contract.ts`).
//!
//! Типы ядра сериализуются прямо в IPC-ответы, поэтому переименованное поле — это
//! не рефакторинг, а сломанный экран. Здесь ответы осматриваются ровно так, как
//! их увидит фронт: как JSON.

mod common;

use serde_json::{json, Value};
use syncra_core::{GeneratorProfile, SecuritySettings};

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
