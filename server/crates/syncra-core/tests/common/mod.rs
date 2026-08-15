//! Общая обвязка приёмочных тестов ядра.
//!
//! Модуль подключается в каждый тестовый бинарник целиком, и каждому нужна своя
//! часть — отсюда `dead_code`.
#![allow(dead_code)]

use syncra_core::{Core, CoreError, CoreErrorCode, RecordDraft};

pub const MASTER_PASSWORD: &str = "мастер-пароль-1";

/// Свежесозданное открытое хранилище: ровно то состояние, в котором пользователь
/// оказывается сразу после онбординга.
pub fn unlocked() -> Core {
    let mut core = Core::in_memory().expect("хранилище в памяти");
    core.init_vault(MASTER_PASSWORD).expect("init_vault");
    core
}

pub fn draft(service_name: &str, login: &str, password: &str) -> RecordDraft {
    RecordDraft {
        vault_id: None,
        service_name: service_name.to_owned(),
        urls: vec![format!("{}.com", service_name.to_lowercase())],
        login: login.to_owned(),
        account_label: None,
        password: password.to_owned(),
        notes: None,
        totp_secret: None,
    }
}

/// Проверяет код ошибки и заодно печатает сообщение при расхождении — иначе
/// упавший тест сообщает только «не тот вариант enum».
#[track_caller]
pub fn assert_code<T: std::fmt::Debug>(result: Result<T, CoreError>, expected: CoreErrorCode) {
    match result {
        Ok(value) => panic!("ждали {expected:?}, получили Ok({value:?})"),
        Err(error) => assert_eq!(error.code, expected, "ждали {expected:?}, получили {error}"),
    }
}
