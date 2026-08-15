//! Ядро Syncra (§8.3).
//!
//! Здесь живёт всё, что не зависит от платформы и от оболочки: модель данных,
//! криптография, хранилище и состояние замка. Tauri этот крейт не знает — и не
//! должен: `cargo test` проверяет ядро целиком, без webview и без окна.
//!
//! Договор с фронтом — `client/src/core/contract.ts`; исполняемая спецификация
//! поведения — фейк-ядро `client/src/core/mock/index.ts`. Расхождение с ними
//! считается багом ядра, а не «другой трактовкой».
//!
//! **Что этот шаг умеет:** жизненный цикл хранилища (`get_vault_status`,
//! `init_vault`, `unlock`, `lock`), CRUD записей и CRUD секций. Синхронизация,
//! сопряжение, конфликты, генератор, импорт/экспорт, TOTP и PIN — следующие шаги.

pub mod crypto;
pub mod error;
pub mod model;
pub mod session;
pub mod storage;

pub use error::{CoreError, CoreErrorCode, CoreResult};
pub use model::{
    InitVaultResponse, PinStatus, RecordDraft, RecordMeta, RecordPatch, RecordSecrets, SecretField,
    UnlockResponse, Vault, VaultPatch, VaultStatus,
};
pub use session::Core;
