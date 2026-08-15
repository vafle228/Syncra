//! Состояние хранилища и точка входа для всех команд контракта.
//!
//! [`Core`] — это замок: он держит ключ, пока хранилище открыто, и зануляет его
//! при `lock()`. Каждая команда, кроме `get_vault_status` / `init_vault` /
//! `unlock`, проходит через [`Core::key`] — один guard вместо проверки «а мы
//! точно открыты?» в каждом методе.
//!
//! Здесь нет ни Tauri, ни событий: события эмитит оболочка по результату вызова.

use std::path::Path;

use crate::crypto::{self, KdfParams, VaultKey};
use crate::error::{CoreError, CoreResult};
use crate::model::{
    now_iso, InitVaultResponse, IsoDateTime, PinStatus, RecordDraft, RecordMeta, RecordPatch,
    RecordSecrets, UnlockResponse, Vault, VaultPatch, VaultStatus, MASTER_PASSWORD_MIN_LENGTH,
};
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

        let salt = self
            .storage
            .meta_get(schema::META_KDF_SALT)?
            .ok_or_else(|| CoreError::internal("Хранилище повреждено."))?;
        let params: KdfParams = serde_json::from_slice(
            &self
                .storage
                .meta_get(schema::META_KDF_PARAMS)?
                .ok_or_else(|| CoreError::internal("Хранилище повреждено."))?,
        )?;
        let verifier = self
            .storage
            .meta_get(schema::META_VERIFIER)?
            .ok_or_else(|| CoreError::internal("Хранилище повреждено."))?;

        let key = crypto::derive_key(master_password, &salt, &params)?;
        if !crypto::verify(&key, &verifier) {
            return Err(CoreError::invalid_master_password());
        }

        Ok(UnlockResponse {
            unlocked_at: self.accept_key(key),
        })
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
        at
    }

    /// Единственный вход за замок. Порядок проверок важен: несозданное хранилище
    /// не «заперто», и говорить о нём `LOCKED` значило бы отправить UI на экран
    /// входа вместо онбординга.
    fn key(&self) -> CoreResult<&VaultKey> {
        if !self.storage.is_initialized()? {
            return Err(CoreError::not_initialized());
        }
        self.key.as_ref().ok_or_else(CoreError::locked)
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
}
