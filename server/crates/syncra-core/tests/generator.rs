//! Генератор паролей (F6, §6.1).
//!
//! Проверяется договор, а не внутренности: что профиль настраивается один раз и
//! переживает замок, что негодные правила не сохраняются, и что за замком
//! генератор молчит, как и всё остальное содержимое хранилища.

mod common;

use common::{assert_code, MASTER_PASSWORD};
use syncra_core::{Core, CoreErrorCode, GeneratorProfile};

fn phrase_profile() -> GeneratorProfile {
    GeneratorProfile {
        mode: "words".to_owned(),
        words: 5,
        separator: ".".to_owned(),
        ..GeneratorProfile::default()
    }
}

#[test]
fn fresh_vault_starts_with_the_default_profile() {
    let core = common::unlocked();

    assert_eq!(
        core.generator_profile().unwrap(),
        GeneratorProfile::default()
    );
}

#[test]
fn saved_profile_outlives_the_lock() {
    let mut core = common::unlocked();
    let saved = core.save_generator_profile(&phrase_profile()).unwrap();
    assert_eq!(saved, phrase_profile());

    core.lock();
    core.unlock(MASTER_PASSWORD).unwrap();

    // Ради этого профиль и живёт в хранилище: настроил один раз (§6.1).
    assert_eq!(core.generator_profile().unwrap(), phrase_profile());
    // И правила из него действительно применяются.
    let password = core
        .generate_passwords(1, None)
        .unwrap()
        .passwords
        .remove(0);
    assert_eq!(password.split('.').count(), 5);
}

#[test]
fn bad_profile_is_rejected_and_does_not_replace_the_saved_one() {
    let core = common::unlocked();
    core.save_generator_profile(&phrase_profile()).unwrap();

    for broken in [
        GeneratorProfile {
            length: 4, // короче GENERATOR_LIMITS.length.min
            ..GeneratorProfile::default()
        },
        GeneratorProfile {
            words: 99,
            ..GeneratorProfile::default()
        },
        GeneratorProfile {
            separator: "_".to_owned(),
            ..GeneratorProfile::default()
        },
        GeneratorProfile {
            mode: "syllables".to_owned(),
            ..GeneratorProfile::default()
        },
    ] {
        assert_code(
            core.save_generator_profile(&broken),
            CoreErrorCode::Validation,
        );
    }

    // Ни одна из неудачных попыток не должна была ничего испортить.
    assert_eq!(core.generator_profile().unwrap(), phrase_profile());
}

#[test]
fn one_off_profile_does_not_touch_the_saved_one() {
    let core = common::unlocked();

    // Предпросмотр на экране настроек: правила разовые, профиль прежний.
    let preview = core.generate_passwords(3, Some(&phrase_profile())).unwrap();
    assert_eq!(preview.passwords.len(), 3);
    assert!(preview.passwords.iter().all(|word| word.contains('.')));

    assert_eq!(
        core.generator_profile().unwrap(),
        GeneratorProfile::default()
    );
}

#[test]
fn count_outside_the_limits_is_rejected() {
    let core = common::unlocked();

    assert_code(core.generate_passwords(0, None), CoreErrorCode::Validation);
    assert_code(core.generate_passwords(11, None), CoreErrorCode::Validation);

    assert_eq!(core.generate_passwords(1, None).unwrap().passwords.len(), 1);
    assert_eq!(
        core.generate_passwords(10, None).unwrap().passwords.len(),
        10
    );
}

#[test]
fn generated_passwords_are_fresh_every_time() {
    let core = common::unlocked();

    let first = core.generate_passwords(5, None).unwrap();
    let second = core.generate_passwords(5, None).unwrap();

    assert!(first
        .passwords
        .iter()
        .all(|password| !second.passwords.contains(password)));
    // Оценка стойкости зависит от правил, а не от выпавших символов.
    assert_eq!(first.entropy_bits, second.entropy_bits);
    assert!(first.entropy_bits > 0);
}

#[test]
fn generator_is_silent_behind_the_lock() {
    let mut core = common::unlocked();
    core.lock();

    assert_code(core.generator_profile(), CoreErrorCode::Locked);
    assert_code(
        core.save_generator_profile(&GeneratorProfile::default()),
        CoreErrorCode::Locked,
    );
    assert_code(core.generate_passwords(5, None), CoreErrorCode::Locked);
}

#[test]
fn generator_needs_a_vault_at_all() {
    let core = Core::in_memory(common::host()).unwrap();

    assert_code(core.generator_profile(), CoreErrorCode::NotInitialized);
}
