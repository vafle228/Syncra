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
//! `confirm_pairing`, `cancel_pairing`), обнаружение соседей в локальной сети и
//! защищённый транспорт между ними (`net`), а также дифференциальный обмен
//! записями поверх него (`sync`: манифест, дифф, надгробия, `get_sync_status`,
//! `sync_now`), а также разрешение расхождений между устройствами
//! (`sync::conflicts`: `list_conflicts`, `resolve_conflict`,
//! `get_conflict_secret`), а также перенос данных наружу и внутрь (`transfer`:
//! `export_csv`, `export_backup`, `delete_export`, `restore_backup`,
//! `begin_import`, `commit_import`, `cancel_import`). TOTP и энролмент PIN —
//! следующие шаги.

pub mod crypto;
pub mod error;
pub mod generator;
pub mod model;
pub mod net;
pub mod pairing;
pub mod security;
pub mod session;
pub mod storage;
pub mod sync;
pub mod transfer;
pub mod trust;

pub use error::{CoreError, CoreErrorCode, CoreResult};
pub use generator::{GeneratedPasswords, GeneratorProfile};
pub use model::{
    ChangeMasterPasswordResponse, ConflictField, ConflictSecrets, ConflictSide, ConflictVersion,
    Device, DeviceKind, ExportFile, HostDevice, ImportOptions, ImportPreview, ImportPreviewRow,
    ImportResult, ImportRowStatus, ImportSource, InitVaultResponse, PairingHandshake, PairingOffer,
    PairingResult, PeerFound, PinStatus, QrMatrix, RecordConflict, RecordDraft, RecordMeta,
    RecordPatch, RecordSecrets, RestoreBackupResult, SecretField, SyncPhase, SyncStatus,
    UnlockResponse, Vault, VaultPatch, VaultStatus,
};
pub use net::{CoreEvent, Node, NodeSettings};
pub use security::{SecuritySettings, SecuritySettingsPatch};
pub use session::Core;
