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
//! **Что это ядро умеет:** жизненный цикл хранилища (`get_vault_status`,
//! `init_vault`, `unlock`, `lock`), CRUD записей и CRUD секций, генератор
//! паролей, настройки безопасности с автоблокировкой, смену мастер-пароля,
//! идентичность устройства с таблицей доверия (`list_devices`, `revoke_device`)
//! и сопряжение по QR (`get_pairing_payload`, `submit_paired_key`,
//! `confirm_pairing`, `cancel_pairing`). Синхронизация, конфликты,
//! импорт/экспорт, TOTP и энролмент PIN — следующие шаги.

pub mod crypto;
pub mod error;
pub mod generator;
pub mod model;
pub mod pairing;
pub mod security;
pub mod session;
pub mod storage;
pub mod trust;

pub use error::{CoreError, CoreErrorCode, CoreResult};
pub use generator::{GeneratedPasswords, GeneratorProfile};
pub use model::{
    ChangeMasterPasswordResponse, Device, DeviceKind, HostDevice, InitVaultResponse,
    PairingHandshake, PairingOffer, PairingResult, PinStatus, QrMatrix, RecordDraft, RecordMeta,
    RecordPatch, RecordSecrets, SecretField, UnlockResponse, Vault, VaultPatch, VaultStatus,
};
pub use security::{SecuritySettings, SecuritySettingsPatch};
pub use session::Core;
