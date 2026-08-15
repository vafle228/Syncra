//! Состояние хранилища и точка входа для всех команд контракта.
//!
//! [`Core`] — это замок: он держит ключ, пока хранилище открыто, и зануляет его
//! при `lock()`. Каждая команда, кроме `get_vault_status` / `init_vault` /
//! `unlock`, проходит через [`Core::key`] — один guard вместо проверки «а мы
//! точно открыты?» в каждом методе. Там же отмечается активность для
//! автоблокировки: пройти за замок мимо этого места нельзя.
//!
//! Здесь нет ни Tauri, ни событий: события эмитит оболочка по результату вызова.

use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::crypto::{self, KdfParams, VaultKey};
use crate::error::{CoreError, CoreResult};
use crate::generator::{self, GeneratedPasswords, GeneratorProfile, Rules};
use crate::model::{
    now_iso, ChangeMasterPasswordResponse, InitVaultResponse, IsoDateTime, PinStatus, RecordDraft,
    RecordMeta, RecordPatch, RecordSecrets, UnlockResponse, Vault, VaultPatch, VaultStatus,
    MASTER_PASSWORD_MIN_LENGTH, PIN_LENGTH,
};
use crate::security::{SecuritySettings, SecuritySettingsPatch};
use crate::storage::{records, schema, vaults, Storage};

/// Имя и цвет первой секции: одна секция по умолчанию заводится сразу, потому
/// что выбирать между секциями до того, как они заведены, не из чего.
const INITIAL_VAULT_NAME: &str = "Личное";
const INITIAL_VAULT_COLOR: &str = "indigo";

pub struct Core {
    storage: Storage,
    /// `None` — хранилище заперто. Ключ живёт только здесь.
    key: Option<VaultKey>,
    unlocked_at: Option<IsoDateTime>,
    /// Когда ядро последний раз что-то делало для пользователя. Отсюда считается
    /// автоблокировка. `Cell` — потому что отмечает активность [`Core::key`],
    /// а он берёт `&self`: иначе каждая читающая команда требовала бы `&mut`.
    last_activity: Cell<Instant>,
    /// Копия `autolock_ms` из настроек. Сторож в оболочке спрашивает про срок
    /// раз в секунду, и ходить за ним в БД каждый раз незачем.
    autolock_ms: i64,
}

impl Core {
    pub fn open(path: &Path) -> CoreResult<Self> {
        Ok(Self::wrap(Storage::open(path)?))
    }

    pub fn in_memory() -> CoreResult<Self> {
        Ok(Self::wrap(Storage::open_in_memory()?))
    }

    fn wrap(storage: Storage) -> Self {
        Self {
            storage,
            key: None,
            unlocked_at: None,
            last_activity: Cell::new(Instant::now()),
            autolock_ms: SecuritySettings::default().autolock_ms,
        }
    }

    // -----------------------------------------------------------------------
    // Жизненный цикл (F3)
    // -----------------------------------------------------------------------

    /// Работает всегда — и на запертом, и на несозданном хранилище: с этого
    /// запроса начинается запуск UI.
    pub fn status(&self) -> CoreResult<VaultStatus> {
        Ok(VaultStatus {
            initialized: self.storage.is_initialized()?,
            unlocked: self.key.is_some(),
            unlocked_at: self.unlocked_at.clone(),
            // Быстрый вход — следующий шаг; поле обязано быть в ответе уже сейчас,
            // иначе экран блокировки не решит, что рисовать, до первой отрисовки.
            pin: PinStatus::not_enrolled(),
        })
    }

    /// Создать хранилище (§3.9). Успешная инициализация оставляет его открытым:
    /// заставлять вводить пароль второй раз подряд бессмысленно.
    pub fn init_vault(&mut self, master_password: &str) -> CoreResult<InitVaultResponse> {
        if self.storage.is_initialized()? {
            return Err(CoreError::already_initialized());
        }
        if master_password.chars().count() < MASTER_PASSWORD_MIN_LENGTH {
            return Err(CoreError::validation(format!(
                "Мастер-пароль короче {MASTER_PASSWORD_MIN_LENGTH} символов."
            )));
        }

        let salt = crypto::random_salt()?;
        let params = KdfParams::default();
        let key = crypto::derive_key(master_password, &salt, &params)?;
        let verifier = crypto::seal_verifier(&key)?;

        let initialized_at = now_iso();
        let device_id = uuid::Uuid::new_v4().to_string();
        let params_json = serde_json::to_vec(&params)?;

        // Одной транзакцией: наполовину созданное хранилище — это файл, который
        // не открывается ничем и не пересоздаётся, потому что «уже существует».
        let tx = self.storage.conn_mut().transaction()?;
        for (key_name, value) in [
            (
                schema::META_SCHEMA_VERSION,
                schema::SCHEMA_VERSION.to_string().into_bytes(),
            ),
            (schema::META_KDF_SALT, salt.to_vec()),
            (schema::META_KDF_PARAMS, params_json),
            (schema::META_DEVICE_ID, device_id.into_bytes()),
            (schema::META_CREATED_AT, initialized_at.clone().into_bytes()),
            // Умолчания настроек кладутся сразу, а не при первом чтении: иначе
            // «ещё не сохраняли» и «сохранили ровно умолчания» неотличимы.
            (
                schema::META_GENERATOR_PROFILE,
                serde_json::to_vec(&GeneratorProfile::default())?,
            ),
            (
                schema::META_SECURITY_SETTINGS,
                serde_json::to_vec(&SecuritySettings::default())?,
            ),
            // Проверочное значение пишется последним: именно по нему хранилище
            // считается созданным (`Storage::is_initialized`).
            (schema::META_VERIFIER, verifier),
        ] {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                rusqlite::params![key_name, value],
            )?;
        }
        vaults::create(&tx, INITIAL_VAULT_NAME, INITIAL_VAULT_COLOR, true)?;
        tx.commit()?;

        let unlocked_at = self.accept_key(key);
        Ok(InitVaultResponse {
            initialized_at,
            unlocked_at,
        })
    }

    pub fn unlock(&mut self, master_password: &str) -> CoreResult<UnlockResponse> {
        if !self.storage.is_initialized()? {
            return Err(CoreError::not_initialized());
        }

        let salt = self.meta_required(schema::META_KDF_SALT)?;
        let params: KdfParams =
            serde_json::from_slice(&self.meta_required(schema::META_KDF_PARAMS)?)?;
        let verifier = self.meta_required(schema::META_VERIFIER)?;

        let key = crypto::derive_key(master_password, &salt, &params)?;
        if !crypto::verify(&key, &verifier) {
            return Err(CoreError::invalid_master_password());
        }

        Ok(UnlockResponse {
            unlocked_at: self.accept_key(key),
        })
    }

    /// Быстрый вход по PIN (F13).
    ///
    /// **Граница шага:** завести PIN пока нечем — команды энролмента в контракте
    /// нет (фейк-ядро заводит его ручкой для дева, `mock/index.ts:221`). Поэтому
    /// ядро повторяет поведение мока для незаведённого PIN и отвечает отказом с
    /// объяснением. Успешного исхода здесь пока не бывает, отсюда и `()` вместо
    /// формы ответа: пустой тип честнее, чем никогда не возвращаемая структура.
    pub fn unlock_with_pin(&self, pin: &str) -> CoreResult<()> {
        if !self.storage.is_initialized()? {
            return Err(CoreError::not_initialized());
        }

        // Не та форма запроса — виноват UI, и это настоящая ошибка.
        if pin.chars().count() != PIN_LENGTH || !pin.chars().all(|char| char.is_ascii_digit()) {
            return Err(CoreError::validation(format!(
                "PIN — это {PIN_LENGTH} цифры."
            )));
        }

        Err(CoreError::validation(
            "Быстрый вход на этом устройстве не настроен.",
        ))
    }

    /// Смена мастер-пароля (F13) — то есть перешифровка хранилища новым ключом.
    ///
    /// Старый пароль обязателен, хотя хранилище уже открыто: перешифровать его —
    /// не то же самое, что им пользоваться, и второе не даёт права на первое
    /// (`contract.ts:271`).
    pub fn change_master_password(
        &mut self,
        current_master_password: &str,
        new_master_password: &str,
    ) -> CoreResult<ChangeMasterPasswordResponse> {
        self.key()?;

        let salt = self.meta_required(schema::META_KDF_SALT)?;
        let params: KdfParams =
            serde_json::from_slice(&self.meta_required(schema::META_KDF_PARAMS)?)?;
        let verifier = self.meta_required(schema::META_VERIFIER)?;

        let current_key = crypto::derive_key(current_master_password, &salt, &params)?;
        if !crypto::verify(&current_key, &verifier) {
            return Err(CoreError::invalid_master_password());
        }

        if new_master_password.chars().count() < MASTER_PASSWORD_MIN_LENGTH {
            return Err(CoreError::validation(format!(
                "Мастер-пароль короче {MASTER_PASSWORD_MIN_LENGTH} символов."
            )));
        }
        if new_master_password == current_master_password {
            return Err(CoreError::validation(
                "Новый мастер-пароль совпадает с текущим.",
            ));
        }

        // Соль новая: одинаковая соль у старого и нового пароля означала бы, что
        // тот, кто видел файл раньше, уже посчитал половину работы по подбору.
        let new_salt = crypto::random_salt()?;
        let new_params = KdfParams::default();
        let new_key = crypto::derive_key(new_master_password, &new_salt, &new_params)?;
        let new_verifier = crypto::seal_verifier(&new_key)?;
        let new_params_json = serde_json::to_vec(&new_params)?;

        // Одной транзакцией: наполовину перешифрованное хранилище не открывается
        // ни старым паролем, ни новым.
        let tx = self.storage.conn_mut().transaction()?;
        records::rekey_all(&tx, &current_key, &new_key)?;
        for (key_name, value) in [
            (schema::META_KDF_SALT, new_salt.to_vec()),
            (schema::META_KDF_PARAMS, new_params_json),
            (schema::META_VERIFIER, new_verifier),
        ] {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key_name, value],
            )?;
        }
        tx.commit()?;

        // Хранилище остаётся открытым — пароль только что подтвердили, — но уже
        // новым ключом. `unlocked_at` при этом прежний: сеанс не начинался заново.
        self.key = Some(new_key);
        self.last_activity.set(Instant::now());

        Ok(ChangeMasterPasswordResponse {
            changed_at: now_iso(),
            // Доверенных устройств пока нет вовсе: сопряжение — следующий шаг.
            devices_to_update: 0,
        })
    }

    fn meta_required(&self, key: &str) -> CoreResult<Vec<u8>> {
        self.storage
            .meta_get(key)?
            .ok_or_else(|| CoreError::internal("Хранилище повреждено."))
    }

    /// Запереть. Ключ зануляется вместе с `VaultKey`, расшифрованных данных ядро
    /// между вызовами не держит — держать нечего.
    pub fn lock(&mut self) {
        self.key = None;
        self.unlocked_at = None;
    }

    pub fn is_unlocked(&self) -> bool {
        self.key.is_some()
    }

    fn accept_key(&mut self, key: VaultKey) -> IsoDateTime {
        let at = now_iso();
        self.key = Some(key);
        self.unlocked_at = Some(at.clone());
        self.last_activity.set(Instant::now());
        self.autolock_ms = self.stored_autolock_ms();
        at
    }

    /// Единственный вход за замок. Порядок проверок важен: несозданное хранилище
    /// не «заперто», и говорить о нём `LOCKED` значило бы отправить UI на экран
    /// входа вместо онбординга.
    ///
    /// Здесь же отмечается активность для автоблокировки — в одном месте на все
    /// команды, чтобы ни одна не могла «забыть» это сделать.
    fn key(&self) -> CoreResult<&VaultKey> {
        if !self.storage.is_initialized()? {
            return Err(CoreError::not_initialized());
        }
        let key = self.key.as_ref().ok_or_else(CoreError::locked)?;
        self.last_activity.set(Instant::now());
        Ok(key)
    }

    // -----------------------------------------------------------------------
    // Автоблокировка (F13). Считает ЯДРО — своего таймера у фронта нет
    // (`contract.ts:539`): два таймера — это две правды о том, заперто ли
    // хранилище, и расходиться они начнут в первый же день.
    // -----------------------------------------------------------------------

    /// Пора ли запирать. Время приходит аргументом, а не берётся изнутри: иначе
    /// это нельзя проверить тестом, не проспав в нём пять минут.
    pub fn autolock_due(&self, now: Instant) -> bool {
        if self.key.is_none() {
            return false;
        }
        now.duration_since(self.last_activity.get())
            >= Duration::from_millis(self.autolock_ms as u64)
    }

    /// Срок автоблокировки из настроек, а при нечитаемых настройках — умолчание.
    ///
    /// Единственное место, где испорченная строка настроек не считается ошибкой:
    /// отпирание хранилища не должно зависеть от неё вовсе. Команда
    /// `get_security_settings` про ту же поломку честно ответит ошибкой.
    fn stored_autolock_ms(&self) -> i64 {
        self.stored_security_settings()
            .unwrap_or_default()
            .autolock_ms
    }

    // -----------------------------------------------------------------------
    // Записи (F4, F5)
    // -----------------------------------------------------------------------

    pub fn list_records(
        &self,
        vault_id: Option<&str>,
        include_deleted: bool,
    ) -> CoreResult<Vec<RecordMeta>> {
        self.key()?;
        records::list(self.storage.conn(), vault_id, include_deleted)
    }

    pub fn get_secret(&self, record_id: &str) -> CoreResult<RecordSecrets> {
        let key = self.key()?;
        records::secrets(self.storage.conn(), key, record_id)
    }

    pub fn create_record(&self, draft: &RecordDraft) -> CoreResult<RecordMeta> {
        let key = self.key()?;
        let conn = self.storage.conn();

        // Секцию проверяем до записи: чужой `vault_id` из UI не должен создать
        // запись, которой нет ни в одной секции.
        let vault = match draft.vault_id.as_deref() {
            Some(id) => vaults::find(conn, id)?,
            None => vaults::default_vault(conn)?,
        };

        records::create(conn, key, draft, &vault.vault_id)
    }

    pub fn update_record(&self, record_id: &str, patch: &RecordPatch) -> CoreResult<RecordMeta> {
        let key = self.key()?;
        let conn = self.storage.conn();

        if let Some(vault_id) = patch.vault_id.as_deref() {
            vaults::find(conn, vault_id)?;
        }

        records::update(conn, key, record_id, patch)
    }

    pub fn delete_record(&self, record_id: &str) -> CoreResult<RecordMeta> {
        self.key()?;
        records::delete(self.storage.conn(), record_id)
    }

    // -----------------------------------------------------------------------
    // Секции (F7)
    // -----------------------------------------------------------------------

    pub fn list_vaults(&self) -> CoreResult<Vec<Vault>> {
        // Состав секций — содержимое хранилища: за замком его не показываем.
        self.key()?;
        vaults::list(self.storage.conn())
    }

    pub fn create_vault(&self, name: &str, color: &str) -> CoreResult<Vault> {
        self.key()?;
        vaults::create(self.storage.conn(), name, color, false)
    }

    pub fn update_vault(&self, vault_id: &str, patch: &VaultPatch) -> CoreResult<Vault> {
        self.key()?;
        vaults::update(self.storage.conn(), vault_id, patch)
    }

    pub fn set_vault_sync(&self, vault_id: &str, sync: bool) -> CoreResult<Vault> {
        self.key()?;
        vaults::set_sync(self.storage.conn(), vault_id, sync)
    }

    pub fn set_default_vault(&mut self, vault_id: &str) -> CoreResult<Vec<Vault>> {
        self.key()?;
        vaults::set_default(self.storage.conn_mut(), vault_id)
    }

    pub fn delete_vault(&mut self, vault_id: &str) -> CoreResult<Vec<Vault>> {
        self.key()?;
        vaults::delete(self.storage.conn_mut(), vault_id)
    }

    // -----------------------------------------------------------------------
    // Генератор паролей (F6)
    // -----------------------------------------------------------------------

    /// Профиль — часть содержимого хранилища, поэтому за замком: на закрытом
    /// хранилище рассказывать, какими правилами пользуется владелец, незачем.
    pub fn generator_profile(&self) -> CoreResult<GeneratorProfile> {
        self.key()?;
        Ok(self.stored_generator_rules()?.to_profile())
    }

    pub fn save_generator_profile(
        &self,
        profile: &GeneratorProfile,
    ) -> CoreResult<GeneratorProfile> {
        self.key()?;

        let normalized = Rules::parse(profile)?.to_profile();
        self.storage.meta_set(
            schema::META_GENERATOR_PROFILE,
            &serde_json::to_vec(&normalized)?,
        )?;
        Ok(normalized)
    }

    pub fn generate_passwords(
        &self,
        count: i64,
        profile: Option<&GeneratorProfile>,
    ) -> CoreResult<GeneratedPasswords> {
        self.key()?;

        // Разовые правила (предпросмотр на экране настроек) проверяются так же
        // строго, как сохраняемые, но профиль в хранилище НЕ меняют.
        let rules = match profile {
            Some(profile) => Rules::parse(profile)?,
            None => self.stored_generator_rules()?,
        };
        generator::generate(&rules, count)
    }

    /// Сохранённый профиль. Если в хранилище лежит профиль, не проходящий нынешние
    /// правила (его мог записать более старое ядро), берутся умолчания: запертый
    /// навсегда генератор — худший исход, чем забытая настройка.
    fn stored_generator_rules(&self) -> CoreResult<Rules> {
        let stored = match self.storage.meta_get(schema::META_GENERATOR_PROFILE)? {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => GeneratorProfile::default(),
        };

        Rules::parse(&stored).or_else(|_| Rules::parse(&GeneratorProfile::default()))
    }

    // -----------------------------------------------------------------------
    // Настройки безопасности (F13)
    // -----------------------------------------------------------------------

    /// За замком, как и профиль генератора: рассказывать про настройки защиты
    /// закрытого хранилища незачем.
    pub fn security_settings(&self) -> CoreResult<SecuritySettings> {
        self.key()?;
        self.stored_security_settings()
    }

    pub fn save_security_settings(
        &mut self,
        patch: &SecuritySettingsPatch,
    ) -> CoreResult<SecuritySettings> {
        self.key()?;

        let next = self.stored_security_settings()?.patched(patch)?;
        self.storage
            .meta_set(schema::META_SECURITY_SETTINGS, &serde_json::to_vec(&next)?)?;
        // Новый срок действует сразу, а не со следующего отпирания: человек его
        // только что выбрал и ждёт, что так и будет.
        self.autolock_ms = next.autolock_ms;
        Ok(next)
    }

    fn stored_security_settings(&self) -> CoreResult<SecuritySettings> {
        match self.storage.meta_get(schema::META_SECURITY_SETTINGS)? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Ok(SecuritySettings::default()),
        }
    }
}
