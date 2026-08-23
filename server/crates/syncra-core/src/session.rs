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
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use subtle::ConstantTimeEq;

use crate::crypto::{self, KdfParams, VaultKey};
use crate::error::{CoreError, CoreResult};
use crate::generator::{self, GeneratedPasswords, GeneratorProfile, Rules};
use crate::model::{
    now_iso, ChangeMasterPasswordResponse, ConflictSecrets, ConflictSide, Device, HostDevice,
    InitVaultResponse, IsoDateTime, PairingHandshake, PairingOffer, PairingResult, PinStatus,
    RecordConflict, RecordDraft, RecordId, RecordMeta, RecordPatch, RecordSecrets, SecretField,
    UnlockResponse, Vault, VaultPatch, VaultStatus, MASTER_PASSWORD_MIN_LENGTH, PIN_LENGTH,
};
use crate::net::{NetIdentity, RemotePeer};
use crate::pairing;
use crate::security::{SecuritySettings, SecuritySettingsPatch};
use crate::storage::{records, schema, vaults, Storage};
use crate::sync::{self, conflicts};
use crate::trust;

/// Имя и цвет первой секции: одна секция по умолчанию заводится сразу, потому
/// что выбирать между секциями до того, как они заведены, не из чего.
const INITIAL_VAULT_NAME: &str = "Личное";
const INITIAL_VAULT_COLOR: &str = "indigo";

pub struct Core {
    storage: Storage,
    /// Что оболочка знает про машину: имя хоста и тип устройства. Приходит
    /// готовым, как и путь к файлу, — сама ядро этого не узнаёт (§8.2).
    host: HostDevice,
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
    /// Код, который это устройство показывает прямо сейчас (F8, §2.2).
    offer: Option<OfferState>,
    /// Коды, которые оно показывало раньше, и до какого момента про каждый из
    /// них ещё стоит помнить: по ним отвечаем «истёк», а не «незнакомый».
    /// Человек читает со второго экрана устаревший QR чаще, чем ошибается
    /// символом. Дальше срока помнить незачем — и нечего копить (см.
    /// [`Core::forget_expired_pairings`]).
    retired_codes: HashMap<String, IsoDateTime>,
    /// Прочитанные пейлоады, ждущие, пока человек сверит слова.
    handshakes: HashMap<String, PendingPeer>,
    /// Сопряжение, о котором осталось рассказать второй стороне (S3).
    ///
    /// Забирается узлом сети СРАЗУ после `confirm_pairing` и уже без замка:
    /// доставка идёт по сети, а держать под мьютексом сетевое ожидание нельзя.
    announcement: Option<PairingAnnouncement>,
}

/// Показанный код и всё, что за ним стоит. В БД не попадает: код даёт право
/// забрать копию хранилища, и переживать замок он не должен.
struct OfferState {
    code: String,
    expires_at: IsoDateTime,
    /// Строка кода целиком. Наружу, в IPC, она не уходит (там матрица), но
    /// внутри процесса живёт: код переносят не только камерой.
    payload: String,
    /// Приватная половина одноразового ключа сеанса.
    ///
    /// Ею подписывается ECDH канала сопряжения (S3): прочитавший QR сверяет
    /// половину из приветствия с той, что была в пейлоаде, и убеждается, что
    /// говорит с тем самым экраном, а не с кем-то посередине (§2.2).
    session_secret: x25519_dalek::StaticSecret,
    /// Сколько раз по этому офферу не угадали код.
    ///
    /// Шесть символов — это тридцать бит, и в локальной сети они перебираются
    /// за секунды. Длину кода увеличивать нельзя (его диктуют вслух), поэтому
    /// защита здесь: после [`PAIRING_ATTEMPT_LIMIT`] промахов оффер сгорает, и
    /// перебор упирается не в арифметику, а в человека, который должен показать
    /// новый код.
    attempts: u32,
}

/// Сопряжение, состоявшееся здесь, о котором надо рассказать той стороне,
/// которая показывала код (§2.2). Она ничего не вызывала и иначе не узнает.
#[derive(Debug, Clone)]
pub struct PairingAnnouncement {
    /// Код показавшей стороны. Из него на каждом соединении считается
    /// обязательство; сам код в сеть не уходит.
    pub code: String,
    /// Докуда имеет смысл пытаться: после этого срока оффер на той стороне
    /// всё равно не примет.
    pub expires_at: IsoDateTime,
    /// Одноразовый ключ сеанса из прочитанного пейлоада. По нему узнают тот
    /// самый экран среди прочих соседей.
    pub session_public: [u8; pairing::SESSION_PUBLIC_LEN],
}

/// Сколько промахов по коду терпит один показанный код.
///
/// Пять — это заметно меньше, чем нужно перебору, и заметно больше, чем
/// ошибётся человек: неверный символ он исправит с первого-второго раза.
pub const PAIRING_ATTEMPT_LIMIT: u32 = 5;

/// Прочитанное устройство, ждущее подтверждения человеком. В `devices` попадёт
/// только после сверки слов — иначе сверять было бы уже нечего.
struct PendingPeer {
    body: pairing::PairingBody,
    fingerprint_words: Vec<String>,
    expires_at: IsoDateTime,
    /// Код той стороны. Нужен после подтверждения, чтобы доложить ей об успехе.
    code: String,
}

impl Core {
    pub fn open(path: &Path, host: HostDevice) -> CoreResult<Self> {
        Ok(Self::wrap(Storage::open(path)?, host))
    }

    pub fn in_memory(host: HostDevice) -> CoreResult<Self> {
        Ok(Self::wrap(Storage::open_in_memory()?, host))
    }

    fn wrap(storage: Storage, host: HostDevice) -> Self {
        Self {
            storage,
            host,
            key: None,
            unlocked_at: None,
            last_activity: Cell::new(Instant::now()),
            autolock_ms: SecuritySettings::default().autolock_ms,
            offer: None,
            retired_codes: HashMap::new(),
            handshakes: HashMap::new(),
            announcement: None,
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
        // Клонируется до транзакции: `conn_mut()` займёт `self` целиком.
        let host = self.host.clone();

        // Одной транзакцией: наполовину созданное хранилище — это файл, который
        // не открывается ничем и не пересоздаётся, потому что «уже существует».
        //
        // Номера схемы здесь нет намеренно: его пишет `schema::migrate`, и хозяин
        // у него один — иначе две правды о том, какой формат в файле.
        let tx = self.storage.conn_mut().transaction()?;
        for (key_name, value) in [
            (schema::META_KDF_SALT, salt.to_vec()),
            (schema::META_KDF_PARAMS, params_json),
            (schema::META_DEVICE_ID, device_id.clone().into_bytes()),
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
        // Пара ключей заводится здесь же, а не при первом выходе в сеть: без неё
        // устройству нечем представиться, а приватная половина обязана лечь под
        // тот же мастер-пароль, что и пароли (§2.1).
        trust::provision_identity(&tx, &key, &host, &device_id, &initialized_at)?;
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

        // Хранилища схемы 1 создавались без пары ключей: миграция заводит таблицу,
        // но ключ ей взять неоткуда — он запечатывается ключом хранилища, а тот
        // появляется только здесь. Досоздаём при первом же отпирании, иначе такое
        // хранилище навсегда осталось бы без права представляться собой.
        self.ensure_identity(&key)?;

        Ok(UnlockResponse {
            unlocked_at: self.accept_key(key),
        })
    }

    /// Досоздать идентичность, если её нет. Идемпотентна и обычно ничего не делает.
    fn ensure_identity(&mut self, key: &VaultKey) -> CoreResult<()> {
        let device_id = self.device_id()?;
        if trust::has_identity(self.storage.conn(), &device_id)? {
            return Ok(());
        }

        let host = self.host.clone();
        let at = now_iso();
        let tx = self.storage.conn_mut().transaction()?;
        trust::provision_identity(&tx, key, &host, &device_id, &at)?;
        tx.commit()?;
        Ok(())
    }

    /// Идентификатор этого устройства. Лежит в `meta` с самого создания
    /// хранилища — его отсутствие означает испорченный файл, а не «ещё нет».
    fn device_id(&self) -> CoreResult<String> {
        String::from_utf8(self.meta_required(schema::META_DEVICE_ID)?)
            .map_err(|_| CoreError::internal("Хранилище повреждено."))
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

        let device_id = self.device_id()?;
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
        // В ТОЙ ЖЕ транзакции, что и записи: приватный ключ устройства защищён
        // ключом хранилища, и не перешифровать его — значит стереть идентичность
        // устройства, заметив это только в сети и только через неделю.
        trust::rekey_secret(&tx, &current_key, &new_key)?;
        // И там же — отложенные версии из `conflicts`: они защищены тем же
        // ключом хранилища, и не тронуть их значило бы потерять приехавшую
        // сторону спора, узнав об этом только на экране разрешения (§5.5).
        conflicts::rekey_all(&tx, &current_key, &new_key)?;
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
            // Действующие соседи, кроме себя: отозванному обновление не поедет —
            // ему вообще больше ничего не поедет (§2.3).
            devices_to_update: trust::active_peer_count(self.storage.conn(), &device_id)?,
        })
    }

    fn meta_required(&self, key: &str) -> CoreResult<Vec<u8>> {
        self.storage
            .meta_get(key)?
            .ok_or_else(|| CoreError::internal("Хранилище повреждено."))
    }

    /// Запереть. Ключ зануляется вместе с `VaultKey`, расшифрованных данных ядро
    /// между вызовами не держит — держать нечего.
    ///
    /// Заодно забывается всё сопряжение: показанный код — это право забрать
    /// копию хранилища, и переживать замок оно не должно (`mock/index.ts:615`).
    pub fn lock(&mut self) {
        self.key = None;
        self.unlocked_at = None;
        self.offer = None;
        self.retired_codes.clear();
        self.handshakes.clear();
        self.announcement = None;
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

    /// Тот же вход за замок, но БЕЗ отметки активности.
    ///
    /// Существует ровно для сетевого слоя (S3). Сеть работает сама по себе:
    /// анонс, ответы соседям, пробы раз в пятнадцать секунд. Если бы всё это
    /// шло через [`Core::key`], хранилище никогда бы не заперлось само —
    /// автоблокировка считает бездействие ПОЛЬЗОВАТЕЛЯ, а фоновый обмен по
    /// сети бездействия не отменяет. Человек ушёл от компьютера, а хранилище
    /// стоит открытым, потому что телефон в кармане исправно отвечает на
    /// ping, — это ровно та поломка, которую здесь не должно быть возможно
    /// написать случайно.
    fn key_quiet(&self) -> CoreResult<&VaultKey> {
        if !self.storage.is_initialized()? {
            return Err(CoreError::not_initialized());
        }
        self.key.as_ref().ok_or_else(CoreError::locked)
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

        // Обе записи — одной транзакцией. Падение между ними оставило бы
        // конфликт на надгробии: видимого вреда нет (`list_conflicts` такую
        // строку пропустит), но строка-сирота осталась бы в БД навсегда.
        let tx = self.storage.conn().unchecked_transaction()?;
        let meta = records::delete(&tx, record_id)?;
        // Спорить о версиях удалённой записи не о чем: конфликт снимается
        // вместе с ней (§5.5, `mock/index.ts:1023`). Надгробие поедет дальше и
        // так — удаление важнее любого расхождения (§5.4).
        conflicts::drop_for(&tx, record_id)?;
        tx.commit()?;
        Ok(meta)
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
    // Доверенные устройства (F9, §2.3)
    // -----------------------------------------------------------------------

    /// Список доверенных устройств, включая отозванные.
    ///
    /// За замком: рассказывать на закрытом хранилище, что у владельца есть ещё
    /// и телефон, незачем.
    pub fn list_devices(&self) -> CoreResult<Vec<Device>> {
        self.key()?;
        trust::list(self.storage.conn(), &self.device_id()?)
    }

    /// Отзыв доступа (§2.3).
    ///
    /// Вместе со строкой доверия стираются и отметки обмена: если отозванный
    /// ноутбук когда-нибудь вернут новым сопряжением, хранилище он должен
    /// получить заново. Довериться отметкам, сделанным до отзыва, — значит
    /// молча не довезти всё, что изменилось, пока он был отозван.
    pub fn revoke_device(&self, device_id: &str) -> CoreResult<Device> {
        self.key()?;
        let device = trust::revoke(self.storage.conn(), &self.device_id()?, device_id)?;
        self.forget_sync_state(device_id)?;
        Ok(device)
    }

    // -----------------------------------------------------------------------
    // Сопряжение (F8, §2.2)
    // -----------------------------------------------------------------------

    /// Показать код второму устройству.
    ///
    /// За замком: код даёт право забрать копию хранилища, а на запертом
    /// хранилище такого права нет ни у кого.
    ///
    /// Каждый вызов — НОВЫЙ код: одноразовый ключ на то и одноразовый. Прежний
    /// перестаёт годиться сразу, а не доживает свой срок.
    pub fn get_pairing_payload(&mut self) -> CoreResult<PairingOffer> {
        let keypair = {
            let key = self.key()?;
            trust::keypair(self.storage.conn(), key)?
        };
        let device_id = self.device_id()?;

        // Заодно подмести за собой: и отработавшие коды, и прочитанные
        // пейлоады живут по три минуты, а уходили до сих пор только вместе с
        // замком. Место для чистки одно — то, из которого они и берутся.
        self.forget_expired_pairings(Utc::now());
        if let Some(previous) = self.offer.take() {
            self.retired_codes
                .insert(previous.code, previous.expires_at);
        }

        let code = pairing::manual_code()?;
        let session_secret = pairing::session_secret()?;
        let body = pairing::PairingBody {
            device_id,
            name: self.host.name.clone(),
            kind: self.host.kind,
            public_key: keypair.public_key(),
            session_public: pairing::session_public(&session_secret),
        };

        let payload = pairing::encode(&body, &code, &keypair.signing_key())?;
        let qr = pairing::qr_matrix(&payload)?;

        let session_id = uuid::Uuid::new_v4().to_string();
        let expires_at = pairing::expires_at(Utc::now());
        self.offer = Some(OfferState {
            code: code.clone(),
            expires_at: expires_at.clone(),
            payload,
            session_secret,
            attempts: 0,
        });

        Ok(PairingOffer {
            session_id,
            qr,
            manual_code: code,
            expires_at,
        })
    }

    /// Строка кода, который это устройство показывает прямо сейчас.
    ///
    /// Существует отдельно от [`Core::get_pairing_payload`] намеренно: через
    /// границу IPC уходит только матрица QR — в пейлоаде ключи, и JS-строкой им
    /// существовать незачем (`contract.ts:636`). Команды под этот метод нет и не
    /// будет; он для тех, кто переносит код внутри процесса, минуя камеру: так
    /// его читают тесты, и так же его прочитает «сохранить код в файл» (F12),
    /// который экран чтения кода уже предлагает открыть.
    pub fn shown_pairing_payload(&self) -> Option<&str> {
        self.offer.as_ref().map(|offer| offer.payload.as_str())
    }

    /// Отдать ядру то, что прочитали со второго устройства.
    ///
    /// Разбирает ядро: UI не знает формата и не отличает полный пейлоад из QR от
    /// шести символов, набранных руками (`contract.ts:704`). Устройство после
    /// этого вызова ещё НЕ доверенное — сначала человек сверяет слова.
    pub fn submit_paired_key(&mut self, payload: &str) -> CoreResult<PairingHandshake> {
        let local_public = {
            let key = self.key()?;
            trust::keypair(self.storage.conn(), key)?.public_key()
        };
        let this_device_id = self.device_id()?;
        let now = Utc::now();

        let parsed = pairing::parse(payload)?;
        let code = parsed.code().to_owned();

        // Свой же код, который уже не действует, — самая частая ошибка: человек
        // читает со второго экрана устаревший QR.
        let own_offer = self.offer.as_ref().filter(|offer| offer.code == code);
        if self.retired_codes.contains_key(&code)
            || own_offer.is_some_and(|offer| pairing::is_expired(&offer.expires_at, now))
        {
            return Err(CoreError::pairing_expired(
                "Этот код больше не действует. Покажите новый на втором устройстве.",
            ));
        }
        if own_offer.is_some() {
            return Err(CoreError::validation(
                "Это код этого же устройства. Нужен код второго — с его экрана.",
            ));
        }

        let body = match parsed {
            // ГРАНИЦА ШАГА. Шесть символов не несут ключа и нести не могут: это
            // имя сеанса, по которому устройство ищут в локальной сети, а поиска
            // ещё нет (S3). Ответ останется правдой и после него — рядом с этим
            // кодом никого не нашлось; давать его тогда будет сам поиск.
            pairing::Parsed::Code(_) => {
                return Err(CoreError::not_found(
                    "Устройство с этим кодом не найдено рядом. Отсканируйте QR-код \
                     со второго устройства или откройте файл с кодом.",
                ))
            }
            pairing::Parsed::Payload { body, .. } => body,
        };

        // Свой собственный пейлоад с чужого экрана: код другой, а ключ наш.
        if body.public_key == local_public || body.device_id == this_device_id {
            return Err(CoreError::validation(
                "Это код этого же устройства. Нужен код второго — с его экрана.",
            ));
        }

        // Слова — из ключей ОБЕИХ сторон: у человека посередине ключи другие, а
        // значит и слова другие. Возвращаются ДО записи в `devices`.
        let fingerprint_words = trust::pair_fingerprint(&local_public, &body.public_key);
        let handshake = PairingHandshake {
            session_id: uuid::Uuid::new_v4().to_string(),
            peer_name: body.name.clone(),
            peer_kind: body.kind,
            fingerprint_words: fingerprint_words.clone(),
            expires_at: pairing::expires_at(now),
        };

        self.handshakes.insert(
            handshake.session_id.clone(),
            PendingPeer {
                body,
                fingerprint_words,
                expires_at: handshake.expires_at.clone(),
                code,
            },
        );
        Ok(handshake)
    }

    /// Человек сверил слова на двух экранах (§2.2) — записываем устройство.
    pub fn confirm_pairing(&mut self, session_id: &str) -> CoreResult<PairingResult> {
        self.key()?;
        let started = Instant::now();

        // Сеанс забирается до проверки срока: истёкший сеанс закрыт в обе
        // стороны, и предлагать подтвердить его второй раз незачем.
        let pending = self
            .handshakes
            .remove(session_id)
            .ok_or_else(|| CoreError::not_found("Сеанс сопряжения не найден."))?;
        if pairing::is_expired(&pending.expires_at, Utc::now()) {
            return Err(CoreError::pairing_expired(
                "Сеанс сопряжения истёк. Начните заново.",
            ));
        }

        let this_device_id = self.device_id()?;
        let device = trust::upsert_peer(
            self.storage.conn(),
            &trust::PairedPeer {
                device_id: &pending.body.device_id,
                name: &pending.body.name,
                kind: pending.body.kind,
                public_key: &pending.body.public_key,
                fingerprint_words: &pending.fingerprint_words,
            },
            &this_device_id,
            &now_iso(),
        )?;
        // Сопряжение — точка отсчёта: старых отметок с этим `device_id` быть не
        // должно, иначе первичный перенос молча пропустит половину хранилища.
        self.forget_sync_state(&pending.body.device_id)?;

        // Второй стороне об этом рассказать некому, кроме нас: она показывала
        // код и ничего не вызывала. Доставку делает узел сети — уже без замка.
        self.announcement = Some(PairingAnnouncement {
            code: pending.code,
            expires_at: pending.expires_at,
            session_public: pending.body.session_public,
        });

        Ok(PairingResult {
            device,
            // ГРАНИЦА ШАГА: переносить записи некуда, пока нет манифеста и
            // диффа (S4). Ноль здесь — правда о состоянии, а не заглушка.
            records_transferred: 0,
            duration_ms: started.elapsed().as_millis() as i64,
        })
    }

    /// Слова не совпали или передумали: сеанс закрывается, ключ не запоминается.
    ///
    /// Идемпотентно — по «Отмене» можно нажать дважды.
    pub fn cancel_pairing(&mut self, session_id: &str) -> CoreResult<()> {
        self.key()?;
        self.handshakes.remove(session_id);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Что нужно сетевому слою (S3)
    //
    // Все методы этого раздела ходят за замок через `key_quiet`: их зовёт
    // фоновый поток, а не человек, и продлевать ими сеанс нельзя.
    // -----------------------------------------------------------------------

    /// Открыто ли хранилище — в том же порядке проверок, что и [`Core::key`]
    /// (`NOT_INITIALIZED` раньше `LOCKED`).
    ///
    /// Нужна командам, которые перед работой с ядром идут в сеть: про замок
    /// человек должен узнать сразу, а не после того, как узел отходит все
    /// таймауты впустую. Активность отмечается — это действие человека, а не
    /// фоновая проба.
    pub fn guard_unlocked(&self) -> CoreResult<()> {
        self.key().map(|_| ())
    }

    /// Чем это устройство представляется в сети.
    pub fn net_identity(&self) -> CoreResult<NetIdentity> {
        let key = self.key_quiet()?;
        Ok(NetIdentity {
            device_id: self.device_id()?,
            host: self.host.clone(),
            keypair: trust::keypair(self.storage.conn(), key)?,
        })
    }

    /// Доверять ли этому ключу подписи (§2.1, §2.3).
    ///
    /// Единственный вопрос, который вообще решает, пускать соседа: ни адреса,
    /// ни имени, ни MAC в нём нет.
    pub fn authorize_peer(&self, signing_public: &[u8; 32]) -> CoreResult<Option<Device>> {
        self.key_quiet()?;
        trust::authorized_peer(self.storage.conn(), &self.device_id()?, signing_public)
    }

    /// Отметить, что доверенное устройство только что было на связи (§5.1).
    pub fn mark_peer_seen(&self, device_id: &str) -> CoreResult<Option<Device>> {
        self.key_quiet()?;
        trust::touch_last_seen(
            self.storage.conn(),
            &self.device_id()?,
            device_id,
            &now_iso(),
        )
    }

    /// Одноразовый ключ сеанса показанного сейчас кода.
    ///
    /// Им подписывается ECDH канала сопряжения: прочитавший QR сверит его с
    /// тем, что было в пейлоаде. Нет активного оффера — нет и ключа, и канал
    /// сопряжения строится на обычном эфемерном.
    pub fn offer_session_secret(&self) -> Option<x25519_dalek::StaticSecret> {
        self.offer
            .as_ref()
            .filter(|offer| !pairing::is_expired(&offer.expires_at, Utc::now()))
            .map(|offer| offer.session_secret.clone())
    }

    /// Ответить на «покажи пейлоад сеанса» из сети (F8, ручной ввод кода).
    ///
    /// `None` означает отказ, и все причины отказа неотличимы снаружи
    /// намеренно: подбирающему коды незачем знать, теплее или холоднее.
    pub fn serve_pairing_lookup(
        &mut self,
        transcript: &[u8],
        commitment: &[u8],
    ) -> CoreResult<Option<String>> {
        self.key_quiet()?;

        let Some(offer) = self.offer.as_mut() else {
            return Ok(None);
        };
        if pairing::is_expired(&offer.expires_at, Utc::now()) {
            return Ok(None);
        }
        // Сравнение постоянного времени: попыток по офферу всего пять, и
        // таймингом здесь ничего не набрать, — но `==` над секретоподобным
        // значением создаёт прецедент, который потом скопируют в место похуже.
        if bool::from(pairing::code_commitment(transcript, &offer.code).ct_eq(commitment)) {
            return Ok(Some(offer.payload.clone()));
        }

        // Промах. Считаем его и, если попытки кончились, сжигаем код: он ушёл
        // в перебор, и продолжать показывать его на экране опасно.
        offer.attempts += 1;
        if offer.attempts >= PAIRING_ATTEMPT_LIMIT {
            if let Some(burned) = self.offer.take() {
                self.retired_codes.insert(burned.code, burned.expires_at);
            }
        }
        Ok(None)
    }

    /// Принять «я сверил слова и записал тебя» от прочитавшей стороны.
    ///
    /// Это обратная сторона сопряжения (§2.2): здесь показывали код, здесь
    /// ничего не вызывали, и записать соседа в доверенные надо по его же
    /// сообщению. Подтверждения на экране эта сторона не спрашивает — команды
    /// для этого в контракте нет. Держится всё на четырёх вещах: код уехал
    /// внеполосно (QR), живёт три минуты, терпит [`PAIRING_ATTEMPT_LIMIT`]
    /// промахов и сгорает после первого же успеха.
    ///
    /// `channel_signing` — ключ, которым собеседник ТОЛЬКО ЧТО подписал
    /// рукопожатие. Публичный ключ соседа складывается из него и присланной
    /// половины обмена ключами: подставить чужой ключ в этом месте нельзя,
    /// потому что подпись канала сделана именно им.
    pub fn accept_remote_pairing(
        &mut self,
        transcript: &[u8],
        commitment: &[u8],
        peer: &RemotePeer<'_>,
        channel_signing: &[u8; 32],
    ) -> CoreResult<Option<PairingResult>> {
        let started = Instant::now();
        let local_public = {
            let key = self.key_quiet()?;
            trust::keypair(self.storage.conn(), key)?.public_key()
        };

        let Some(offer) = self.offer.as_ref() else {
            return Ok(None);
        };
        if pairing::is_expired(&offer.expires_at, Utc::now())
            || !bool::from(pairing::code_commitment(transcript, &offer.code).ct_eq(commitment))
        {
            // Промах считается тем же счётчиком, что и у поиска: перебирать
            // код можно и через эту дверь.
            return self.miss_pairing_attempt();
        }

        let mut public_key = [0u8; crypto::DEVICE_PUBLIC_LEN];
        public_key[..32].copy_from_slice(channel_signing);
        public_key[32..].copy_from_slice(peer.agreement_public);

        let this_device_id = self.device_id()?;
        if public_key == local_public || peer.device_id == this_device_id {
            return Ok(None);
        }

        // Слова считаются из ключей обеих сторон и сортируются, поэтому здесь
        // выйдет ровно то же, что человек сверил на втором экране.
        let fingerprint_words = trust::pair_fingerprint(&local_public, &public_key);
        let device = trust::upsert_peer(
            self.storage.conn(),
            &trust::PairedPeer {
                device_id: peer.device_id,
                name: peer.name,
                kind: peer.kind,
                public_key: &public_key,
                fingerprint_words: &fingerprint_words,
            },
            &this_device_id,
            &now_iso(),
        )?;
        self.forget_sync_state(peer.device_id)?;

        // Код сделал свою работу и больше не действует: одноразовый ключ на то
        // и одноразовый.
        if let Some(used) = self.offer.take() {
            self.retired_codes.insert(used.code, used.expires_at);
        }

        Ok(Some(PairingResult {
            device,
            records_transferred: 0,
            duration_ms: started.elapsed().as_millis() as i64,
        }))
    }

    fn miss_pairing_attempt(&mut self) -> CoreResult<Option<PairingResult>> {
        if let Some(offer) = self.offer.as_mut() {
            offer.attempts += 1;
            if offer.attempts >= PAIRING_ATTEMPT_LIMIT {
                if let Some(burned) = self.offer.take() {
                    self.retired_codes.insert(burned.code, burned.expires_at);
                }
            }
        }
        Ok(None)
    }

    /// Забыть сеансы сопряжения, чей срок вышел.
    ///
    /// Растут оба списка не быстро — границу задаёт человек, нажимающий
    /// «показать код», — но сжимались они только в [`Core::lock`], а хранилище
    /// бывает открытым сутками. Время приходит аргументом по той же причине,
    /// что и в `autolock_due`: иначе это нельзя проверить, не проспав три
    /// минуты в тесте.
    fn forget_expired_pairings(&mut self, now: DateTime<Utc>) {
        self.retired_codes
            .retain(|_, until| !pairing::is_expired(until, now));
        self.handshakes
            .retain(|_, pending| !pairing::is_expired(&pending.expires_at, now));
    }

    /// Забрать сопряжение, о котором осталось доложить второй стороне.
    ///
    /// Забирается один раз: доставка идёт по сети и под замком её держать
    /// нельзя.
    pub fn take_pairing_announcement(&mut self) -> Option<PairingAnnouncement> {
        self.announcement.take()
    }

    // -----------------------------------------------------------------------
    // Что нужно синхронизации (S4, §5.3)
    //
    // Тот же порядок, что и у сетевых методов выше: вход за замок через
    // `key_quiet`, потому что зовёт всё это фоновый обмен, а не человек.
    // Продлевать сеанс обменом с телефоном в кармане нельзя.
    // -----------------------------------------------------------------------

    /// Чем это хранилище отчитывается перед соседом (§5.3, шаг 1).
    pub fn sync_manifest(&self) -> CoreResult<sync::Manifest> {
        self.key_quiet()?;
        sync::manifest(self.storage.conn())
    }

    /// Что посылать и что просить по манифесту соседа (§5.3, шаг 2).
    pub fn sync_plan(
        &self,
        device_id: &str,
        remote: &sync::Manifest,
    ) -> CoreResult<sync::SyncPlan> {
        self.key_quiet()?;
        sync::plan(self.storage.conn(), device_id, remote)
    }

    /// Собрать дифф, распечатав секреты своим ключом (§5.3, шаг 3).
    ///
    /// Открытый текст отсюда уходит в сеть — и только в неё: границу IPC он не
    /// пересекает, четвёртой команды Закона №1 не появилось.
    pub fn sync_export(&self, ids: &[RecordId]) -> CoreResult<Vec<sync::SyncRecord>> {
        let key = self.key_quiet()?;
        sync::export(self.storage.conn(), key, ids)
    }

    /// Применить приехавший дифф — одной транзакцией.
    ///
    /// Возвращает то, что правда легло: чужая версия старше нашей не трогается,
    /// запись локальной секции не принимается вовсе (§4.2, §5.2), а
    /// разошедшаяся откладывается в спор и живой записи не касается (§5.5).
    /// Поднятые расхождения едут во втором поле — про них сеть шлёт
    /// `conflict_raised`, уже без замка.
    pub fn sync_apply(
        &mut self,
        device_id: &str,
        incoming: &[sync::SyncRecord],
    ) -> CoreResult<sync::Applied> {
        // Ключ клонируется до транзакции: `conn_mut()` займёт `self` целиком.
        let key = self.key_quiet()?.clone();

        let tx = self.storage.conn_mut().transaction()?;
        let applied = sync::apply(&tx, &key, device_id, incoming)?;
        tx.commit()?;
        Ok(applied)
    }

    /// Что «сошлось» с точки зрения стороны, которая чужого манифеста не
    /// видела: инициатор круга знает только, что у него попросили и что ему
    /// прислали.
    pub fn sync_settled_after(
        &self,
        wanted: &[RecordId],
        received: &[RecordId],
    ) -> CoreResult<Vec<(RecordId, i64)>> {
        self.key_quiet()?;
        sync::settled_after(self.storage.conn(), wanted, received)
    }

    /// Отметить, на какой версии сошлись с устройством.
    pub fn sync_note(&self, device_id: &str, agreed: &[(RecordId, i64)]) -> CoreResult<()> {
        self.key_quiet()?;
        sync::note(self.storage.conn(), device_id, agreed)
    }

    /// Круг закончился успехом: запомнить, когда это было.
    ///
    /// Одна строка на хранилище, а не на устройство: индикатор спрашивает
    /// «когда синхронизировались в последний раз», а не «когда — с ноутбуком».
    pub fn sync_finish(&self) -> CoreResult<IsoDateTime> {
        self.key_quiet()?;
        let at = now_iso();
        self.storage
            .meta_set(schema::META_LAST_SYNC_AT, at.as_bytes())?;
        Ok(at)
    }

    pub fn last_sync_at(&self) -> CoreResult<Option<IsoDateTime>> {
        self.key_quiet()?;
        match self.storage.meta_get(schema::META_LAST_SYNC_AT)? {
            Some(raw) => Ok(String::from_utf8(raw).ok()),
            None => Ok(None),
        }
    }

    /// Записи, которые ещё не уехали ни на одно действующее устройство.
    pub fn pending_records(&self) -> CoreResult<Vec<RecordId>> {
        self.key_quiet()?;
        sync::pending(self.storage.conn(), &self.device_id()?)
    }

    /// Забыть отметки обмена с устройством: отзыв и (повторное) сопряжение
    /// начинают перенос с нуля.
    fn forget_sync_state(&self, device_id: &str) -> CoreResult<()> {
        sync::forget_device(self.storage.conn(), device_id)
    }

    /// Собрать один конфликт — для события `conflict_raised`.
    ///
    /// За замок через `key_quiet`: зовёт это сеть сразу после круга, а не
    /// человек. `None` — запись успела исчезнуть, пока событие собиралось.
    pub fn build_conflict(&self, record_id: &str) -> CoreResult<Option<RecordConflict>> {
        let key = self.key_quiet()?;
        conflicts::build(self.storage.conn(), key, &self.device_id()?, record_id)
    }

    // -----------------------------------------------------------------------
    // Конфликты (F11, §5.5)
    //
    // Здесь, в отличие от всего сетевого выше, вход за замок обычный: это
    // человек за экраном разрешения, и его действия автоблокировку отменяют.
    // -----------------------------------------------------------------------

    /// Расхождения, ждущие решения.
    ///
    /// Список, а не счётчик, и приходит он отдельно от `SyncStatus`: две правды
    /// об одном числе рано или поздно разъедутся (`contract.ts:793`).
    pub fn list_conflicts(&self) -> CoreResult<Vec<RecordConflict>> {
        let key = self.key()?;
        conflicts::list(self.storage.conn(), key, &self.device_id()?)
    }

    /// Оставить одну версию целиком (§5.5).
    ///
    /// Проигравшая **не склеивается** с победившей по полям: §5.5 обещает выбор
    /// записи, а не сборку из двух.
    ///
    /// Новая версия — `max(местная, приехавшая) + 1`, и это не перестраховка:
    /// возьми мы `местная + 1`, проигравшая вернулась бы следующим кругом как
    /// более свежая, и человек выбирал бы заново до бесконечности.
    ///
    /// Отметка обмена при этом двигается до версии соседа: «я её видел и учёл».
    /// Без этого следующий же круг поднял бы тот же спор — обе стороны опять
    /// оказались бы дальше общего предка.
    pub fn resolve_conflict(
        &mut self,
        record_id: &str,
        side: ConflictSide,
    ) -> CoreResult<RecordMeta> {
        let key = self.key()?.clone();

        let stored = conflicts::open_for(self.storage.conn(), record_id)?
            .ok_or_else(|| CoreError::not_found("Конфликт версий не найден."))?;
        let local = conflicts::local_side(self.storage.conn(), &key, record_id)?
            .ok_or_else(|| CoreError::not_found("Запись не найдена."))?;

        let resolved_at = now_iso();
        let version = local.version.max(stored.version) + 1;

        let tx = self.storage.conn_mut().transaction()?;
        match side {
            // Местная версия остаётся как есть — двигается только счётчик.
            // Метаданные и секреты не переписываются: переписывать их значениями
            // из них же было бы лишней работой с шифротекстом.
            ConflictSide::Local => {
                tx.execute(
                    "UPDATE records SET version = ?2, updated_at = ?3 WHERE record_id = ?1",
                    rusqlite::params![record_id, version, resolved_at],
                )?;
            }
            ConflictSide::Remote => {
                let secrets = stored.secrets(&key)?;
                // Секцию приехавшей версии могли к этому моменту удалить — тогда
                // запись остаётся там, где лежит: секцию удаляли уже с ней внутри.
                let vault_id = match vaults::find_optional(&tx, &stored.vault_id)? {
                    Some(vault) => vault.vault_id,
                    None => local.vault_id.clone(),
                };
                // `password_updated_at` двигается только вместе с самим паролем:
                // это «когда пароль правда сменили», а не «когда запись трогали»
                // (§4.1).
                let password_updated_at = if secrets.password == local.secrets.password {
                    local.password_updated_at.clone()
                } else {
                    resolved_at.clone()
                };

                tx.execute(
                    "UPDATE records SET
                         vault_id = ?2, service_name = ?3, urls = ?4, login = ?5,
                         account_label = ?6, password_ct = ?7, notes_ct = ?8, totp_ct = ?9,
                         version = ?10, updated_at = ?11, password_updated_at = ?12
                     WHERE record_id = ?1",
                    rusqlite::params![
                        record_id,
                        vault_id,
                        stored.service_name,
                        serde_json::to_string(&stored.urls)?,
                        stored.login,
                        stored.account_label,
                        // Перепечатываются под ОБЫЧНЫЙ AAD: приехавшая версия
                        // становится настоящей записью и должна читаться как
                        // запись, а не как отложенная сторона спора.
                        records::seal_field(
                            &key,
                            record_id,
                            SecretField::Password,
                            secrets.password.as_deref()
                        )?,
                        records::seal_field(
                            &key,
                            record_id,
                            SecretField::Notes,
                            secrets.notes.as_deref()
                        )?,
                        records::seal_field(
                            &key,
                            record_id,
                            SecretField::TotpSecret,
                            secrets.totp_secret.as_deref()
                        )?,
                        version,
                        resolved_at,
                        password_updated_at,
                    ],
                )?;
            }
        }

        conflicts::drop_for(&tx, record_id)?;
        sync::note(
            &tx,
            &stored.device_id,
            &[(record_id.to_owned(), stored.version)],
        )?;
        tx.commit()?;

        records::find_meta(self.storage.conn(), record_id)
    }

    /// Одно секретное поле обеих версий сразу — вторая из трёх команд Закона №1.
    ///
    /// Обе стороны в одном ответе: сравнить одно значение с другим — весь смысл
    /// действия (§5.5, «ручная пересборка»). Правила `get_secret` действуют
    /// здесь целиком: разово, по нажатию, наружу не кэшируется.
    pub fn conflict_secret(
        &self,
        record_id: &str,
        field: SecretField,
    ) -> CoreResult<ConflictSecrets> {
        let key = self.key()?;

        let stored = conflicts::open_for(self.storage.conn(), record_id)?
            .ok_or_else(|| CoreError::not_found("Конфликт версий не найден."))?;
        let local = conflicts::local_side(self.storage.conn(), key, record_id)?
            .ok_or_else(|| CoreError::not_found("Запись не найдена."))?;

        Ok(ConflictSecrets {
            local: local.secrets.field(field),
            remote: stored.secrets(key)?.field(field),
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::HostDevice;

    fn unlocked() -> Core {
        let mut core = Core::in_memory(HostDevice::desktop("Стенд")).expect("хранилище в памяти");
        core.init_vault("мастер-пароль-1").expect("init_vault");
        core
    }

    #[test]
    fn expired_pairing_sessions_do_not_pile_up() {
        let mut core = unlocked();

        // Человек нажимал «показать код» трижды: два прошлых кода отработали и
        // помнятся только затем, чтобы ответить «истёк», а не «незнакомый».
        for _ in 0..3 {
            core.get_pairing_payload().expect("код сопряжения");
        }
        assert_eq!(core.retired_codes.len(), 2);

        // Три минуты спустя помнить о них незачем — и до сих пор они уходили
        // только вместе с замком, а хранилище бывает открытым сутками.
        core.forget_expired_pairings(Utc::now() + chrono::Duration::minutes(10));
        assert!(core.retired_codes.is_empty());
        assert!(core.handshakes.is_empty());

        // Действующий оффер чистка не трогает: он ещё на экране.
        assert!(core.offer.is_some());
        core.forget_expired_pairings(Utc::now());
        assert!(core.offer.is_some());
    }
}
