//! Ошибки ядра в той форме, в какой их ждёт фронт.
//!
//! `client/src/core/errors.ts:33` (`toCoreError`) принимает ТОЛЬКО объект
//! `{ code, message }` с кодом из `CORE_ERROR_CODES`; всё остальное схлопывается
//! в безликое «Ядро вернуло непредвиденную ошибку». Поэтому форма здесь — часть
//! договора, а не деталь реализации.
//!
//! `message` показывается пользователю как есть. В нём не должно быть ни путей к
//! хранилищу, ни ключей, ни мастер-пароля, ни текста ошибок SQLite — внутренние
//! подробности сюда не просачиваются намеренно.

use std::fmt;

use serde::Serialize;

/// Коды из `CORE_ERROR_CODES` (`client/src/core/contract.ts:137`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CoreErrorCode {
    /// Хранилище заперто: нужен unlock.
    Locked,
    /// Неверный мастер-пароль.
    InvalidMasterPassword,
    /// Хранилище ещё не создано.
    NotInitialized,
    /// `init_vault` по уже созданному хранилищу.
    AlreadyInitialized,
    /// Запись/сущность не найдена (или является надгробием).
    NotFound,
    /// Запрос не прошёл валидацию ядра.
    Validation,
    /// Код сопряжения устарел.
    PairingExpired,
    /// Непредвиденная ошибка ядра.
    Internal,
}

impl CoreErrorCode {
    /// Строка на проводе — она же и в тестах, чтобы не сверять через serde.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Locked => "LOCKED",
            Self::InvalidMasterPassword => "INVALID_MASTER_PASSWORD",
            Self::NotInitialized => "NOT_INITIALIZED",
            Self::AlreadyInitialized => "ALREADY_INITIALIZED",
            Self::NotFound => "NOT_FOUND",
            Self::Validation => "VALIDATION",
            Self::PairingExpired => "PAIRING_EXPIRED",
            Self::Internal => "INTERNAL",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoreError {
    pub code: CoreErrorCode,
    pub message: String,
}

impl CoreError {
    pub fn new(code: CoreErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn locked() -> Self {
        Self::new(CoreErrorCode::Locked, "Хранилище заперто.")
    }

    pub fn invalid_master_password() -> Self {
        Self::new(
            CoreErrorCode::InvalidMasterPassword,
            "Неверный мастер-пароль.",
        )
    }

    pub fn not_initialized() -> Self {
        Self::new(
            CoreErrorCode::NotInitialized,
            "Хранилище на этом устройстве ещё не создано.",
        )
    }

    pub fn already_initialized() -> Self {
        Self::new(
            CoreErrorCode::AlreadyInitialized,
            "Хранилище на этом устройстве уже создано.",
        )
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(CoreErrorCode::NotFound, message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(CoreErrorCode::Validation, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(CoreErrorCode::Internal, message)
    }

    /// Часть контракта, которую это ядро ещё не умеет. Отдельный конструктор, а
    /// не просто `internal`, чтобы такие места были видны поиском по одному слову.
    pub fn not_ready() -> Self {
        Self::internal("Эта часть ядра ещё не готова.")
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for CoreError {}

/// Ошибка SQLite наружу не пересказывается: её текст может содержать имена
/// таблиц, значения полей и путь к файлу, а `message` уходит прямо в UI и в логи.
impl From<rusqlite::Error> for CoreError {
    fn from(_: rusqlite::Error) -> Self {
        Self::internal("Не удалось обратиться к хранилищу.")
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(_: serde_json::Error) -> Self {
        Self::internal("Не удалось разобрать данные хранилища.")
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
