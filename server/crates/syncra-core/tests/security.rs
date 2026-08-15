//! Настройки безопасности и автоблокировка (F13).
//!
//! Автоблокировку исполняет ядро, а не фронт (`contract.ts:539`), поэтому здесь
//! же проверяется отсчёт бездействия: время подаётся аргументом, чтобы тест не
//! спал по пять минут.

mod common;

use std::time::{Duration, Instant};

use common::{assert_code, draft, MASTER_PASSWORD};
use syncra_core::{Core, CoreErrorCode, SecuritySettings, SecuritySettingsPatch};

fn patch_autolock(ms: i64) -> SecuritySettingsPatch {
    SecuritySettingsPatch {
        autolock_ms: Some(ms),
        ..SecuritySettingsPatch::default()
    }
}

#[test]
fn fresh_vault_starts_with_the_defaults() {
    let core = common::unlocked();

    assert_eq!(
        core.security_settings().unwrap(),
        SecuritySettings::default()
    );
}

#[test]
fn settings_outlive_the_lock() {
    let mut core = common::unlocked();
    let saved = core
        .save_security_settings(&SecuritySettingsPatch {
            autolock_ms: Some(60_000),
            secret_reveal_ms: Some(120_000),
            ..SecuritySettingsPatch::default()
        })
        .unwrap();

    assert_eq!(saved.autolock_ms, 60_000);
    assert_eq!(saved.secret_reveal_ms, 120_000);
    // Не названное в патче осталось прежним.
    assert_eq!(
        saved.clipboard_clear_ms,
        SecuritySettings::default().clipboard_clear_ms
    );

    core.lock();
    core.unlock(MASTER_PASSWORD).unwrap();
    assert_eq!(core.security_settings().unwrap(), saved);
}

#[test]
fn value_outside_the_steps_saves_nothing() {
    let mut core = common::unlocked();
    core.save_security_settings(&patch_autolock(60_000))
        .unwrap();

    // Первое поле годное, второе — нет: не должно примениться ни одно.
    assert_code(
        core.save_security_settings(&SecuritySettingsPatch {
            autolock_ms: Some(1_800_000),
            clipboard_clear_ms: Some(3_000),
            secret_reveal_ms: None,
        }),
        CoreErrorCode::Validation,
    );

    assert_eq!(core.security_settings().unwrap().autolock_ms, 60_000);
    assert_eq!(
        core.security_settings().unwrap().clipboard_clear_ms,
        SecuritySettings::default().clipboard_clear_ms
    );
}

#[test]
fn settings_are_silent_behind_the_lock() {
    let mut core = common::unlocked();
    core.lock();

    assert_code(core.security_settings(), CoreErrorCode::Locked);
    assert_code(
        core.save_security_settings(&patch_autolock(60_000)),
        CoreErrorCode::Locked,
    );
}

#[test]
fn autolock_comes_due_after_the_chosen_idle_time() {
    let mut core = common::unlocked();
    core.save_security_settings(&patch_autolock(60_000))
        .unwrap();

    let now = Instant::now();
    assert!(!core.autolock_due(now));
    assert!(!core.autolock_due(now + Duration::from_secs(59)));
    // Ровно на сроке — уже пора: «через минуту» не значит «через минуту и чуть-чуть».
    assert!(core.autolock_due(now + Duration::from_secs(60)));
}

#[test]
fn work_pushes_the_autolock_away() {
    let mut core = common::unlocked();
    core.save_security_settings(&patch_autolock(60_000))
        .unwrap();

    let idle_since = Instant::now();
    // Любая команда за замком — это активность; отдельно отмечаться ей не нужно.
    core.create_record(&draft("Github", "octocat", "пароль-1"))
        .unwrap();

    assert!(!core.autolock_due(idle_since + Duration::from_secs(60)));
}

#[test]
fn a_locked_vault_is_never_due() {
    let mut core = common::unlocked();
    core.save_security_settings(&patch_autolock(60_000))
        .unwrap();
    core.lock();

    // Запирать запертое незачем: иначе сторож эмитил бы `locked` каждую секунду.
    assert!(!core.autolock_due(Instant::now() + Duration::from_secs(3_600)));
}

#[test]
fn a_vault_that_was_never_created_is_never_due() {
    let core = Core::in_memory().unwrap();

    assert!(!core.autolock_due(Instant::now() + Duration::from_secs(3_600)));
}

#[test]
fn pin_login_says_plainly_that_it_is_not_set_up() {
    let core = common::unlocked();

    // Не та форма запроса — виноват UI.
    assert_code(core.unlock_with_pin("12"), CoreErrorCode::Validation);
    assert_code(core.unlock_with_pin("abcd"), CoreErrorCode::Validation);
    // Форма верная, но заводить PIN пока нечем: команды энролмента нет в контракте.
    assert_code(core.unlock_with_pin("1234"), CoreErrorCode::Validation);
    assert!(!core.status().unwrap().pin.enrolled);
}
