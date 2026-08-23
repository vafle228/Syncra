//! IPC-команды: тонкие обёртки над `syncra-core`.
//!
//! Имена — из `COMMAND_NAMES` (`client/src/core/contract.ts:1232`). Каждая
//! команда принимает ровно один аргумент `request`, как договорено в шапке
//! контракта: `invoke('list_records', { request })`.
//!
//! Логики здесь нет и быть не должно: всё, что тут появляется, — это разбор
//! запроса, вызов ядра и (для команд замка) событие наружу.

use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use syncra_core::{
    ChangeMasterPasswordResponse, ConflictSecrets, ConflictSide, Core, CoreError, Device,
    GeneratedPasswords, GeneratorProfile, InitVaultResponse, Node, PairingHandshake, PairingOffer,
    PairingResult, RecordConflict, RecordDraft, RecordMeta, RecordPatch, RecordSecrets,
    SecretField, SecuritySettings, SecuritySettingsPatch, SyncStatus, UnlockResponse, Vault,
    VaultPatch, VaultStatus,
};
use tauri::{AppHandle, Emitter, Manager, State};

/// Ядро под замком процесса. Команды синхронные: они короткие, а самая долгая
/// (вывод ключа при `unlock`) выполняется один раз за сеанс.
///
/// `Arc`, а не просто `Mutex`: тем же ядром пользуется сетевой узел из своих
/// потоков (S3). Держит он его через `Weak`, поэтому владельцем остаётся
/// оболочка — но `Arc` для этого нужен здесь.
pub struct CoreState(pub Arc<Mutex<Core>>);

/// Сетевая сторона ядра (S3).
///
/// Команды, которым нужна сеть, идут через узел, а не прямо в ядро: узел умеет
/// отпустить замок на время ожидания, а команда под `core!` — нет.
pub struct NodeState(pub Arc<Node>);

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

#[derive(Deserialize)]
pub struct DeviceIdRequest {
    device_id: String,
}

/// Прочитанное со второго устройства — ОДНОЙ строкой. UI не знает формата и не
/// отличает полный пейлоад из QR от кода, набранного руками: разбирает ядро.
#[derive(Deserialize)]
pub struct SubmitPairedKeyRequest {
    payload: String,
}

#[derive(Deserialize)]
pub struct PairingSessionRequest {
    session_id: String,
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

/// Переложить событие ядра в событие Tauri (S3).
///
/// Ядро про Tauri не знает и знать не должно: оно складывает события в канал, а
/// имя события на проводе и способ доставки — дело оболочки.
pub fn forward(app: &AppHandle, event: syncra_core::CoreEvent) {
    match event {
        syncra_core::CoreEvent::PeerFound(peer) => announce(app, "peer_found", peer),
        syncra_core::CoreEvent::DevicePaired(result) => announce(app, "device_paired", *result),
        syncra_core::CoreEvent::SyncStatus(status) => announce(app, "sync_status", *status),
        syncra_core::CoreEvent::ConflictRaised(conflict) => {
            announce(app, "conflict_raised", *conflict)
        }
    }
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
/// и уронить приложение из фонового потока он не должен. Но и молчать он не
/// имеет права: тихий выход на каждом тике означал бы, что хранилище больше
/// НИКОГДА не запрётся само, — ключ остаётся в памяти процесса, человек уходит
/// от компьютера, а замок не щёлкает. Невозможность проверить срок — это
/// причина запереть, а не причина не запирать.
pub fn autolock_tick(app: &AppHandle) {
    let state = app.state::<CoreState>();
    let poisoned = state.0.is_poisoned();
    let mut core = state.0.lock().unwrap_or_else(PoisonError::into_inner);

    if !poisoned && !core.autolock_due(std::time::Instant::now()) {
        return;
    }
    // Уже заперто — рассказывать не о чем. Без этой проверки отравленный
    // мьютекс слал бы «заперлись» каждую секунду до конца работы приложения.
    if !core.is_unlocked() {
        return;
    }
    core.lock();
    drop(core);

    // Событие уходит уже с отпущенным замком: подписчик наверняка попросит
    // статус в ответ, и держать мьютекс в этот момент незачем.
    //
    // `system`, а не `timeout`: срок здесь ни при чём, заперло не бездействие
    // человека, а поломка (`contract.ts:1287`).
    announce_locked(app, if poisoned { "system" } else { "timeout" });
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
// Доверенные устройства (F9, §2.3)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_devices(state: State<'_, CoreState>) -> Answer<Vec<Device>> {
    core!(state).list_devices()
}

/// Отзыв доступа. Идемпотентен, себя отозвать нельзя — правила в ядре
/// (`syncra_core::trust`), здесь только вызов.
#[tauri::command]
pub fn revoke_device(request: DeviceIdRequest, state: State<'_, CoreState>) -> Answer<Device> {
    core!(state).revoke_device(&request.device_id)
}

// ---------------------------------------------------------------------------
// Сопряжение (F8, §2.2)
// ---------------------------------------------------------------------------

/// Код для второго устройства. Строки пейлоада в ответе нет — только картинка:
/// одноразовому ключу сеанса незачем существовать ещё и JS-строкой.
#[tauri::command]
pub fn get_pairing_payload(state: State<'_, CoreState>) -> Answer<PairingOffer> {
    core!(state).get_pairing_payload()
}

/// Прочитанное со второго устройства.
///
/// Идёт через узел, а не через `core!`: шесть символов, набранных руками, — это
/// имя сеанса, по которому устройство ИЩУТ в локальной сети (§2.2), и держать
/// мьютекс ядра через этот поиск нельзя. Полный пейлоад из QR при этом сети не
/// требует — но UI не знает, что ему подали, и разбирается в этом ядро.
#[tauri::command]
pub fn submit_paired_key(
    request: SubmitPairedKeyRequest,
    node: State<'_, NodeState>,
) -> Answer<PairingHandshake> {
    node.0.submit_paired_key(&request.payload)
}

/// Человек сверил слова на двух экранах и подтвердил (§2.2).
///
/// События `device_paired` отсюда НЕ шлётся: его получает та сторона, которая
/// показывала код и ничего не вызывала. Здесь результат и так возвращается
/// ответом на команду, а второе уведомление было бы эхом.
///
/// Идёт через узел: сверх записи в `devices` тому, кто показывал код, надо
/// доложить об успехе по сети — а это ожидание, и держать через него замок
/// нельзя. Доставка уходит в фон, ответ команды её не ждёт.
#[tauri::command]
pub fn confirm_pairing(
    request: PairingSessionRequest,
    node: State<'_, NodeState>,
) -> Answer<PairingResult> {
    node.0.confirm_pairing(&request.session_id)
}

#[tauri::command]
pub fn cancel_pairing(request: PairingSessionRequest, state: State<'_, CoreState>) -> Answer<()> {
    core!(state).cancel_pairing(&request.session_id)
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
// Синхронизация (F10, §5.1)
// ---------------------------------------------------------------------------

/// Что происходит с синхронизацией прямо сейчас.
///
/// Полный статус, а не дельта: событие `sync_status` устроено так же, и
/// собирать состояние по кусочкам фронту не приходится.
///
/// За замком, как и в фейк-ядре: сеть работает только на отпертом хранилище, и
/// рассказывать про соседей — а тем более про то, сколько записей ждёт
/// отправки, — запертому незачем.
#[tauri::command]
pub fn get_sync_status(
    state: State<'_, CoreState>,
    node: State<'_, NodeState>,
) -> Answer<SyncStatus> {
    require_unlocked(&state)?;
    Ok(node.0.status())
}

/// «Синхронизировать сейчас»: не ждать своей минуты, а обойти соседей и
/// обменяться прямо теперь.
///
/// Ответ круга НЕ ждёт — обмен идёт по сети, а команда должна вернуться сразу.
/// Чем он кончился, расскажет событие `sync_status`; так же устроен и
/// `mock/index.ts::syncNow`.
#[tauri::command]
pub fn sync_now(state: State<'_, CoreState>, node: State<'_, NodeState>) -> Answer<SyncStatus> {
    require_unlocked(&state)?;
    Ok(node.0.sync_now())
}

fn require_unlocked(state: &State<'_, CoreState>) -> Answer<()> {
    if core!(state).is_unlocked() {
        Ok(())
    } else {
        Err(CoreError::locked())
    }
}

// ---------------------------------------------------------------------------
// Конфликты версий (F11, §5.5)
// ---------------------------------------------------------------------------

/// `side` и `field` приезжают строками, а разбирает их ядро.
///
/// Не `#[derive(Deserialize)]` по самому enum: провал serde на границе Tauri —
/// это строка, из которой `toCoreError` слепит безликое «непредвиденная ошибка
/// ядра», а контракт обещает здесь `VALIDATION` с человеческим текстом.
#[derive(Deserialize)]
pub struct ResolveConflictRequest {
    record_id: String,
    side: String,
}

#[derive(Deserialize)]
pub struct GetConflictSecretRequest {
    record_id: String,
    field: String,
}

#[tauri::command]
pub fn list_conflicts(state: State<'_, CoreState>) -> Answer<Vec<RecordConflict>> {
    core!(state).list_conflicts()
}

#[tauri::command]
pub fn resolve_conflict(
    request: ResolveConflictRequest,
    state: State<'_, CoreState>,
) -> Answer<RecordMeta> {
    let side = ConflictSide::parse(&request.side)?;
    core!(state).resolve_conflict(&request.record_id, side)
}

/// Вторая из трёх команд контракта, отдающих открытый текст. Обе стороны сразу:
/// сравнить одно значение с другим — весь смысл действия.
#[tauri::command]
pub fn get_conflict_secret(
    request: GetConflictSecretRequest,
    state: State<'_, CoreState>,
) -> Answer<ConflictSecrets> {
    let field = SecretField::parse(&request.field)?;
    core!(state).conflict_secret(&request.record_id, field)
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
    // Экспорт, импорт, бэкап (F12)
    export_csv,
    export_backup,
    delete_export,
    restore_backup,
    begin_import,
    commit_import,
    cancel_import,
];
