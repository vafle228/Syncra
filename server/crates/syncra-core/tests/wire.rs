//! Форма ответов на проводе — договор с фронтом (`client/src/core/contract.ts`).
//!
//! Типы ядра сериализуются прямо в IPC-ответы, поэтому переименованное поле — это
//! не рефакторинг, а сломанный экран. Здесь ответы шага «генератор и безопасность»
//! осматриваются ровно так, как их увидит фронт: как JSON.

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
