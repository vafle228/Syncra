//! IPC-команды: тонкие обёртки над `syncra-core`.
//!
//! Имена — из `COMMAND_NAMES` (`client/src/core/contract.ts:1232`). Каждая
//! команда принимает ровно один аргумент `request`, как договорено в шапке
//! контракта: `invoke('list_records', { request })`.
//!
//! Логики здесь нет и быть не должно: всё, что тут появляется, — это разбор
//! запроса, вызов ядра и (для команд замка) событие наружу.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use syncra_core::{
    ChangeMasterPasswordResponse, Core, CoreError, GeneratedPasswords, GeneratorProfile,
    InitVaultResponse, RecordDraft, RecordMeta, RecordPatch, RecordSecrets, SecuritySettings,
    SecuritySettingsPatch, UnlockResponse, Vault, VaultPatch, VaultStatus,
};
use tauri::{AppHandle, Emitter, Manager, State};

/// Ядро под замком процесса. Команды синхронные: они короткие, а самая долгая
/// (вывод ключа при `unlock`) выполняется один раз за сеанс.
pub struct CoreState(pub Mutex<Core>);

type Answer<T> = Result<T, CoreError>;

/// Отравленный мьютекс означает панику внутри ядра — состояние хранилища после
/// такого доверия не заслуживает, и притворяться, что всё в порядке, нельзя.
macro_rules! core {
    ($state:expr) => {
        $state
            .0
            .lock()
            .map_err(|_| CoreError::internal("Ядро в несогласованном состоянии."))?
    };
}

// ---------------------------------------------------------------------------
// Формы запросов
// ---------------------------------------------------------------------------

// Команды без полей в запросе не объявляют аргумента вовсе: Tauri разбирает
// payload по именам объявленных параметров, а лишние ключи (`request: {}`,
// который фронт шлёт всем без исключения) просто не читает.

#[derive(Deserialize)]
pub struct MasterPasswordRequest {
    master_password: String,
}

#[derive(Deserialize)]
pub struct PinRequest {
    pin: String,
}

#[derive(Deserialize)]
pub struct ChangeMasterPasswordRequest {
    current_master_password: String,
    new_master_password: String,
}

#[derive(Deserialize)]
pub struct SaveGeneratorProfileRequest {
    profile: GeneratorProfile,
}

#[derive(Deserialize)]
pub struct GeneratePasswordsRequest {
    count: i64,
    /// Разовые правила для предпросмотра. Не указаны — берётся сохранённый профиль.
    #[serde(default)]
    profile: Option<GeneratorProfile>,
}

#[derive(Deserialize)]
pub struct SaveSecuritySettingsRequest {
    patch: SecuritySettingsPatch,
}

#[derive(Deserialize)]
pub struct ListRecordsRequest {
    #[serde(default)]
    vault_id: Option<String>,
    #[serde(default)]
    include_deleted: Option<bool>,
}

#[derive(Deserialize)]
pub struct RecordIdRequest {
    record_id: String,
}

#[derive(Deserialize)]
pub struct CreateRecordRequest {
    draft: RecordDraft,
}

#[derive(Deserialize)]
pub struct UpdateRecordRequest {
    record_id: String,
    patch: RecordPatch,
}

#[derive(Deserialize)]
pub struct CreateVaultRequest {
    name: String,
    color: String,
}

#[derive(Deserialize)]
pub struct UpdateVaultRequest {
    vault_id: String,
    patch: VaultPatch,
}

#[derive(Deserialize)]
pub struct SetVaultSyncRequest {
    vault_id: String,
    sync: bool,
}

#[derive(Deserialize)]
pub struct VaultIdRequest {
    vault_id: String,
}

// ---------------------------------------------------------------------------
// События (`EVENT_NAMES`, contract.ts:1339)
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
struct UnlockedEvent {
    unlocked_at: String,
}

#[derive(Serialize, Clone)]
struct LockedEvent {
    locked_at: String,
    reason: &'static str,
}

/// Событие — не отчёт об успехе команды, а способ сообщить остальному UI.
/// Не доставилось — команда всё равно отработала, и валить её из-за этого нельзя.
fn announce<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: T) {
    let _ = app.emit(event, payload);
}

// ---------------------------------------------------------------------------
// Жизненный цикл хранилища (F3)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_vault_status(state: State<'_, CoreState>) -> Answer<VaultStatus> {
    core!(state).status()
}

#[tauri::command]
pub fn init_vault(
    request: MasterPasswordRequest,
    state: State<'_, CoreState>,
    app: AppHandle,
) -> Answer<InitVaultResponse> {
    let response = core!(state).init_vault(&request.master_password)?;
    announce(
        &app,
        "unlocked",
        UnlockedEvent {
            unlocked_at: response.unlocked_at.clone(),
        },
    );
    Ok(response)
}

#[tauri::command]
pub fn unlock(
    request: MasterPasswordRequest,
    state: State<'_, CoreState>,
    app: AppHandle,
) -> Answer<UnlockResponse> {
    let response = core!(state).unlock(&request.master_password)?;
    announce(
        &app,
        "unlocked",
        UnlockedEvent {
            unlocked_at: response.unlocked_at.clone(),
        },
    );
    Ok(response)
}

#[tauri::command]
pub fn lock(state: State<'_, CoreState>, app: AppHandle) -> Answer<()> {
    core!(state).lock();
    announce_locked(&app, "manual");
    Ok(())
}

fn announce_locked(app: &AppHandle, reason: &'static str) {
    announce(
        app,
        "locked",
        LockedEvent {
            locked_at: syncra_core::model::now_iso(),
            reason,
        },
    );
}

/// Шаг сторожа автоблокировки (F13). Вызывается оболочкой раз в секунду.
///
/// Считает срок ЯДРО — здесь только «спросить и, если пора, запереть». Своего
/// таймера у фронта нет и быть не должно (`contract.ts:539`): два таймера — это
/// две правды о том, заперто ли хранилище.
///
/// Отравленный мьютекс здесь не повод падать: сторож — не команда пользователя,
/// и уронить приложение из фонового потока он не должен.
pub fn autolock_tick(app: &AppHandle) {
    let state = app.state::<CoreState>();
    let Ok(mut core) = state.0.lock() else {
        return;
    };

    if !core.autolock_due(std::time::Instant::now()) {
        return;
    }
    core.lock();
    drop(core);

    // Событие уходит уже с отпущенным замком: подписчик наверняка попросит
    // статус в ответ, и держать мьютекс в этот момент незачем.
    announce_locked(app, "timeout");
}

/// Быстрый вход по PIN (F13). Завести PIN пока нечем — команды энролмента нет в
/// контракте, — поэтому ядро отвечает внятным отказом, а не «непредвиденной
/// ошибкой». Экран блокировки клавиатуру и не покажет: `pin.enrolled` — `false`.
#[tauri::command]
pub fn unlock_with_pin(request: PinRequest, state: State<'_, CoreState>) -> Answer<()> {
    core!(state).unlock_with_pin(&request.pin)
}

/// Смена мастер-пароля: ядро перешифровывает хранилище целиком.
///
/// Событий не шлёт: хранилище как было открыто, так и осталось, и рассказывать
/// остальному UI нечего.
#[tauri::command]
pub fn change_master_password(
    request: ChangeMasterPasswordRequest,
    state: State<'_, CoreState>,
) -> Answer<ChangeMasterPasswordResponse> {
    core!(state).change_master_password(
        &request.current_master_password,
        &request.new_master_password,
    )
}

// ---------------------------------------------------------------------------
// Записи (F4, F5)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_records(
    request: ListRecordsRequest,
    state: State<'_, CoreState>,
) -> Answer<Vec<RecordMeta>> {
    core!(state).list_records(
        request.vault_id.as_deref(),
        request.include_deleted.unwrap_or(false),
    )
}

/// Одна из трёх команд контракта, отдающих открытый текст, — и единственная в
/// этом шаге. Вызывается по явному действию человека.
#[tauri::command]
pub fn get_secret(request: RecordIdRequest, state: State<'_, CoreState>) -> Answer<RecordSecrets> {
    core!(state).get_secret(&request.record_id)
}

#[tauri::command]
pub fn create_record(
    request: CreateRecordRequest,
    state: State<'_, CoreState>,
) -> Answer<RecordMeta> {
    core!(state).create_record(&request.draft)
}

#[tauri::command]
pub fn update_record(
    request: UpdateRecordRequest,
    state: State<'_, CoreState>,
) -> Answer<RecordMeta> {
    core!(state).update_record(&request.record_id, &request.patch)
}

#[tauri::command]
pub fn delete_record(request: RecordIdRequest, state: State<'_, CoreState>) -> Answer<RecordMeta> {
    core!(state).delete_record(&request.record_id)
}

// ---------------------------------------------------------------------------
// Секции (F7)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_vaults(state: State<'_, CoreState>) -> Answer<Vec<Vault>> {
    core!(state).list_vaults()
}

#[tauri::command]
pub fn create_vault(request: CreateVaultRequest, state: State<'_, CoreState>) -> Answer<Vault> {
    core!(state).create_vault(&request.name, &request.color)
}

#[tauri::command]
pub fn update_vault(request: UpdateVaultRequest, state: State<'_, CoreState>) -> Answer<Vault> {
    core!(state).update_vault(&request.vault_id, &request.patch)
}

#[tauri::command]
pub fn set_vault_sync(request: SetVaultSyncRequest, state: State<'_, CoreState>) -> Answer<Vault> {
    core!(state).set_vault_sync(&request.vault_id, request.sync)
}

#[tauri::command]
pub fn set_default_vault(
    request: VaultIdRequest,
    state: State<'_, CoreState>,
) -> Answer<Vec<Vault>> {
    core!(state).set_default_vault(&request.vault_id)
}

#[tauri::command]
pub fn delete_vault(request: VaultIdRequest, state: State<'_, CoreState>) -> Answer<Vec<Vault>> {
    core!(state).delete_vault(&request.vault_id)
}

// ---------------------------------------------------------------------------
// Генератор паролей (F6)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_generator_profile(state: State<'_, CoreState>) -> Answer<GeneratorProfile> {
    core!(state).generator_profile()
}

#[tauri::command]
pub fn save_generator_profile(
    request: SaveGeneratorProfileRequest,
    state: State<'_, CoreState>,
) -> Answer<GeneratorProfile> {
    core!(state).save_generator_profile(&request.profile)
}

/// Вторая и последняя команда этого ядра, отдающая пароли открытым текстом.
/// Это ещё ничей пароль — но обращение с ним такое же, как с `get_secret`.
#[tauri::command]
pub fn generate_passwords(
    request: GeneratePasswordsRequest,
    state: State<'_, CoreState>,
) -> Answer<GeneratedPasswords> {
    core!(state).generate_passwords(request.count, request.profile.as_ref())
}

// ---------------------------------------------------------------------------
// Настройки безопасности (F13)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_security_settings(state: State<'_, CoreState>) -> Answer<SecuritySettings> {
    core!(state).security_settings()
}

#[tauri::command]
pub fn save_security_settings(
    request: SaveSecuritySettingsRequest,
    state: State<'_, CoreState>,
) -> Answer<SecuritySettings> {
    core!(state).save_security_settings(&request.patch)
}

// ---------------------------------------------------------------------------
// Честные ответы там, где ядру правда нечего сообщить
// ---------------------------------------------------------------------------

/// Синхронизации в этом шаге нет — и `idle` с нулём пиров это не заглушка, а
/// правда о состоянии. Индикатор в шапке рисует «рядом никого» и молчит, вместо
/// того чтобы показывать ошибку на каждом экране.
#[derive(Serialize)]
pub struct SyncStatus {
    phase: &'static str,
    peers_online: u32,
    peer_name: Option<String>,
    pending_records: Vec<String>,
    last_sync_at: Option<String>,
    message: Option<String>,
}

fn idle_sync() -> SyncStatus {
    SyncStatus {
        phase: "idle",
        peers_online: 0,
        peer_name: None,
        pending_records: Vec::new(),
        last_sync_at: None,
        message: None,
    }
}

#[tauri::command]
pub fn get_sync_status() -> Answer<SyncStatus> {
    Ok(idle_sync())
}

#[tauri::command]
pub fn sync_now() -> Answer<SyncStatus> {
    Ok(idle_sync())
}

/// Конфликтов не бывает без синхронизации — пустой список здесь тоже правда.
#[tauri::command]
pub fn list_conflicts() -> Answer<Vec<serde_json::Value>> {
    Ok(Vec::new())
}

// ---------------------------------------------------------------------------
// Ещё не сделанные части контракта
// ---------------------------------------------------------------------------

/// Команды следующих шагов.
///
/// Они зарегистрированы намеренно: незарегистрированную команду Tauri отвергает
/// строкой, а `toCoreError` (`client/src/core/errors.ts:33`) превращает всё, что
/// не `{ code, message }`, в безликое «Ядро вернуло непредвиденную ошибку».
/// Пустая заглушка отвечает понятным текстом вместо этого.
macro_rules! not_ready {
    ($($name:ident),* $(,)?) => {
        $(
            #[tauri::command]
            pub fn $name() -> Answer<()> {
                Err(CoreError::not_ready())
            }
        )*
    };
}

not_ready![
    // Коды подтверждения (фаза 2)
    get_totp_code,
    // Сопряжение и доверие (F8, F9)
    get_pairing_payload,
    submit_paired_key,
    confirm_pairing,
    cancel_pairing,
    list_devices,
    revoke_device,
    // Конфликты (F11)
    resolve_conflict,
    get_conflict_secret,
    // Экспорт, импорт, бэкап (F12)
    export_csv,
    export_backup,
    delete_export,
    restore_backup,
    begin_import,
    commit_import,
    cancel_import,
];
