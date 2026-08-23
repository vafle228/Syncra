//! Записи паролей (§4.1, F4/F5).
//!
//! Единица данных всей системы. В БД от записи лежат открыто только те поля, без
//! которых её нечем найти и не с чем сравнить: идентификаторы, счётчик версий и
//! отметки времени. Секреты (`password`, `notes`, `totp_secret`) шифруются по
//! полям, метаданные (`service_name`, `urls`, `login`, `account_label`) — одним
//! блобом `meta_ct` (S7.1, [`crate::storage::metadata`]).
//!
//! Отсюда следует правило, которого до S7.1 не было: **список записей читается
//! только за замком**. [`list`] по-прежнему физически не может вернуть секрет —
//! `RecordMeta` не умеет его нести, — но ключ хранилища ему теперь нужен так же,
//! как [`secrets`].

use rusqlite::{Connection, OptionalExtension, Row};

use crate::crypto::{self, VaultKey};
use crate::error::{CoreError, CoreResult};
use crate::model::{
    normalize_optional_secret, now_iso, optional_meta, require_non_empty, require_present,
    valid_urls, IsoDateTime, RecordDraft, RecordId, RecordMeta, RecordPatch, RecordSecrets,
    SecretField,
};
use crate::storage::metadata::{self, Place, RecordFields};

/// Запись как она лежит в БД: открытые поля плюс нераспечатанные блобы.
struct StoredRecord {
    record_id: RecordId,
    vault_id: String,
    meta_ct: Option<Vec<u8>>,
    password_ct: Option<Vec<u8>>,
    notes_ct: Option<Vec<u8>>,
    totp_ct: Option<Vec<u8>>,
    version: i64,
    created_at: IsoDateTime,
    updated_at: IsoDateTime,
    password_updated_at: IsoDateTime,
    deleted_at: Option<IsoDateTime>,
}

const COLUMNS: &str = "record_id, vault_id, meta_ct,
                       password_ct, notes_ct, totp_ct,
                       version, created_at, updated_at, password_updated_at, deleted_at";

fn from_row(row: &Row<'_>) -> rusqlite::Result<StoredRecord> {
    Ok(StoredRecord {
        record_id: row.get("record_id")?,
        vault_id: row.get("vault_id")?,
        meta_ct: row.get("meta_ct")?,
        password_ct: row.get("password_ct")?,
        notes_ct: row.get("notes_ct")?,
        totp_ct: row.get("totp_ct")?,
        version: row.get("version")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        password_updated_at: row.get("password_updated_at")?,
        deleted_at: row.get("deleted_at")?,
    })
}

impl StoredRecord {
    fn fields(&self, key: &VaultKey) -> CoreResult<RecordFields> {
        metadata::open(key, Place::Record, &self.record_id, self.meta_ct.as_deref())
    }

    /// Метаданные для UI: распечатанный блоб плюс открытые поля строки.
    fn meta(&self, key: &VaultKey) -> CoreResult<RecordMeta> {
        let fields = self.fields(key)?;
        Ok(RecordMeta {
            record_id: self.record_id.clone(),
            vault_id: self.vault_id.clone(),
            service_name: fields.service_name,
            urls: fields.urls,
            login: fields.login,
            account_label: fields.account_label,
            // Флаги — по наличию шифротекста. Ни одного `open()` ради рамки в карточке.
            has_notes: self.notes_ct.is_some(),
            has_totp: self.totp_ct.is_some(),
            version: self.version,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            password_updated_at: self.password_updated_at.clone(),
            deleted_at: self.deleted_at.clone(),
        })
    }
}

fn fetch(conn: &Connection, record_id: &str) -> CoreResult<Option<StoredRecord>> {
    let sql = format!("SELECT {COLUMNS} FROM records WHERE record_id = ?1");
    Ok(conn.query_row(&sql, [record_id], from_row).optional()?)
}

/// Живая запись или `NOT_FOUND`. Надгробие — тоже «не найдено»: править и
/// открывать удалённую запись нечего (§5.4).
fn fetch_live(conn: &Connection, record_id: &str) -> CoreResult<StoredRecord> {
    match fetch(conn, record_id)? {
        Some(stored) if stored.deleted_at.is_none() => Ok(stored),
        _ => Err(CoreError::not_found("Запись не найдена.")),
    }
}

/// Метаданные одной записи, включая надгробие.
///
/// Отдельно от [`fetch_live`], потому что зовётся оттуда, где запись только что
/// переписали и надо вернуть её новый вид: разрешение конфликта (§5.5).
pub fn find_meta(conn: &Connection, key: &VaultKey, record_id: &str) -> CoreResult<RecordMeta> {
    match fetch(conn, record_id)? {
        Some(stored) => stored.meta(key),
        None => Err(CoreError::not_found("Запись не найдена.")),
    }
}

pub fn list(
    conn: &Connection,
    key: &VaultKey,
    vault_id: Option<&str>,
    include_deleted: bool,
) -> CoreResult<Vec<RecordMeta>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM records
         WHERE (?1 IS NULL OR vault_id = ?1)
           AND (?2 = 1 OR deleted_at IS NULL)"
    );

    let mut stmt = conn.prepare(&sql)?;
    let stored = stmt
        .query_map(
            rusqlite::params![vault_id, include_deleted as i64],
            from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let mut records = stored
        .iter()
        .map(|record| record.meta(key))
        .collect::<CoreResult<Vec<_>>>()?;

    // Порядок ядра — по сервису, затем по логину, без учёта регистра. Сортировка
    // идёт здесь, а не в `ORDER BY`: с S7.1 имя сервиса лежит в файле
    // шифротекстом, и SQL сравнивать его не умеет. Раньше это был выбор, теперь
    // — единственный способ. Стор фронта всё равно пересортировывает у себя
    // через `localeCompare`, поэтому побайтовая параль с JS не нужна; нужен
    // предсказуемый порядок.
    records.sort_by(|a, b| {
        a.service_name
            .to_lowercase()
            .cmp(&b.service_name.to_lowercase())
            .then_with(|| a.login.to_lowercase().cmp(&b.login.to_lowercase()))
    });

    Ok(records)
}

/// `vault_id` приходит уже проверенным: чужая секция из UI не должна создать
/// запись, которой нет ни в одной секции, поэтому её ищут ДО записи.
pub fn create(
    conn: &Connection,
    key: &VaultKey,
    draft: &RecordDraft,
    vault_id: &str,
) -> CoreResult<RecordMeta> {
    let fields = RecordFields {
        service_name: require_non_empty(&draft.service_name, "Сервис")?,
        urls: valid_urls(&draft.urls)?,
        login: require_non_empty(&draft.login, "Логин")?,
        account_label: optional_meta(draft.account_label.as_deref(), "Подпись аккаунта")?,
    };
    let password = require_present(&draft.password, "Пароль")?;

    // ID генерирует ядро, а не UI (§4.1): это ключ синхронизации и разрешения
    // конфликтов, и приходить снаружи он не может.
    let record_id: RecordId = uuid::Uuid::new_v4().to_string();
    let created_at: IsoDateTime = now_iso();

    let meta_ct = metadata::seal(key, Place::Record, &record_id, &fields)?;
    let password_ct = seal_field(key, &record_id, SecretField::Password, Some(&password))?;
    let notes_ct = seal_field(
        key,
        &record_id,
        SecretField::Notes,
        normalize_optional_secret(draft.notes.as_deref(), "Заметки")?.as_deref(),
    )?;
    let totp_ct = seal_field(
        key,
        &record_id,
        SecretField::TotpSecret,
        normalize_optional_secret(draft.totp_secret.as_deref(), "Секрет TOTP")?.as_deref(),
    )?;

    conn.execute(
        "INSERT INTO records (record_id, vault_id, meta_ct,
                              password_ct, notes_ct, totp_ct,
                              version, created_at, updated_at, password_updated_at, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7, ?7, NULL)",
        rusqlite::params![
            record_id,
            vault_id,
            meta_ct,
            password_ct,
            notes_ct,
            totp_ct,
            created_at,
        ],
    )?;

    fetch_live(conn, &record_id)?.meta(key)
}

/// `vault_id` в патче приходит уже проверенным — как и при создании.
pub fn update(
    conn: &Connection,
    key: &VaultKey,
    record_id: &str,
    patch: &RecordPatch,
) -> CoreResult<RecordMeta> {
    let current = fetch_live(conn, record_id)?;
    let current_fields = current.fields(key)?;
    let changed_at = now_iso();

    // Пароль сравниваем по значению, а не по факту прихода поля: форма записи
    // присылает его и тогда, когда человек ничего в нём не менял, а
    // `password_updated_at` — это «когда пароль правда сменили» (§4.1).
    let (password_ct, password_changed) = match patch.password.as_deref() {
        None => (current.password_ct.clone(), false),
        Some(value) => {
            let next = require_present(value, "Пароль")?;
            let previous = open_field(key, record_id, SecretField::Password, &current.password_ct)?
                .unwrap_or_default();
            if next == previous {
                (current.password_ct.clone(), false)
            } else {
                (
                    seal_field(key, record_id, SecretField::Password, Some(&next))?,
                    true,
                )
            }
        }
    };

    let notes_ct = match patch.notes.as_ref() {
        None => current.notes_ct.clone(),
        Some(value) => seal_field(
            key,
            record_id,
            SecretField::Notes,
            normalize_optional_secret(value.as_deref(), "Заметки")?.as_deref(),
        )?,
    };
    let totp_ct = match patch.totp_secret.as_ref() {
        None => current.totp_ct.clone(),
        Some(value) => seal_field(
            key,
            record_id,
            SecretField::TotpSecret,
            normalize_optional_secret(value.as_deref(), "Секрет TOTP")?.as_deref(),
        )?,
    };

    // Метаданные едут одним блобом, поэтому правка любого поля — это новая
    // печать всех четырёх: непришедшие берутся из распечатанных прежних.
    let fields = RecordFields {
        service_name: match patch.service_name.as_deref() {
            Some(value) => require_non_empty(value, "Сервис")?,
            None => current_fields.service_name,
        },
        urls: match patch.urls.as_deref() {
            Some(value) => valid_urls(value)?,
            None => current_fields.urls,
        },
        login: match patch.login.as_deref() {
            Some(value) => require_non_empty(value, "Логин")?,
            None => current_fields.login,
        },
        account_label: match patch.account_label.as_ref() {
            Some(value) => optional_meta(value.as_deref(), "Подпись аккаунта")?,
            None => current_fields.account_label,
        },
    };
    let meta_ct = metadata::seal(key, Place::Record, &current.record_id, &fields)?;

    let vault_id = patch
        .vault_id
        .clone()
        .unwrap_or_else(|| current.vault_id.clone());

    conn.execute(
        "UPDATE records SET
             vault_id = ?2, meta_ct = ?3,
             password_ct = ?4, notes_ct = ?5, totp_ct = ?6,
             version = version + 1, updated_at = ?7, password_updated_at = ?8
         WHERE record_id = ?1",
        rusqlite::params![
            record_id,
            vault_id,
            meta_ct,
            password_ct,
            notes_ct,
            totp_ct,
            changed_at,
            if password_changed {
                &changed_at
            } else {
                &current.password_updated_at
            },
        ],
    )?;

    fetch_live(conn, record_id)?.meta(key)
}

/// Мягкое удаление (§5.4).
///
/// Надгробие хранит `record_id`, `deleted_at` и `version` — и больше ничего:
/// секреты стираются здесь же, а не ждут чистки надгробий через 30 дней.
/// Вместе с ними перестают быть правдой `has_notes` / `has_totp`, и они
/// обнуляются сами — по тому же правилу «флаг = наличие шифротекста».
///
/// Метаданные при этом **остаются** — зашифрованными, как и были. Надгробие
/// едет по сети наравне с записью (§5.3), а `SyncRecord` без имени сервиса на
/// проводе не собирается; после S7.1 они и в файле ничего не выдают.
pub fn delete(conn: &Connection, key: &VaultKey, record_id: &str) -> CoreResult<RecordMeta> {
    fetch_live(conn, record_id)?;
    let deleted_at = now_iso();

    conn.execute(
        "UPDATE records SET
             password_ct = NULL, notes_ct = NULL, totp_ct = NULL,
             version = version + 1, updated_at = ?2, deleted_at = ?2
         WHERE record_id = ?1",
        rusqlite::params![record_id, deleted_at],
    )?;

    match fetch(conn, record_id)? {
        Some(stored) => stored.meta(key),
        None => Err(CoreError::not_found("Запись не найдена.")),
    }
}

/// Единственная команда шага, отдающая открытый текст. Вызывается по явному
/// действию человека, результат наружу не кэшируется.
pub fn secrets(conn: &Connection, key: &VaultKey, record_id: &str) -> CoreResult<RecordSecrets> {
    let stored = fetch_live(conn, record_id)?;

    let password = open_field(key, record_id, SecretField::Password, &stored.password_ct)?
        .ok_or_else(|| CoreError::not_found("Запись не найдена."))?;

    Ok(RecordSecrets {
        password,
        notes: open_field(key, record_id, SecretField::Notes, &stored.notes_ct)?,
        totp_secret: open_field(key, record_id, SecretField::TotpSecret, &stored.totp_ct)?,
    })
}

/// Перешифровать всё содержимое хранилища с одного ключа на другой (F13, смена
/// мастер-пароля).
///
/// «Всё» — это секреты **и метаданные**: с S7.1 под ключом хранилища лежат и те,
/// и другие, и забыть про блоб метаданных значило бы сделать половину записей
/// нечитаемыми новым паролем.
///
/// Открытые поля записи не трогаются вовсе: `version`, `updated_at` и
/// `password_updated_at` остаются прежними. Смена ключа — не изменение записи, и
/// поднимать из-за неё версию значило бы наплодить расхождений для будущего
/// синка на ровном месте (§5.2).
///
/// AAD тоже прежний: `record_id` и имена полей не меняются, поэтому шифротекст
/// остаётся привязан к своему месту так же, как был.
///
/// Работает по транзакции, а не по соединению: наполовину перешифрованное
/// хранилище не открывается ни старым паролем, ни новым.
pub fn rekey_all(tx: &rusqlite::Transaction<'_>, from: &VaultKey, to: &VaultKey) -> CoreResult<()> {
    let mut stmt =
        tx.prepare("SELECT record_id, meta_ct, password_ct, notes_ct, totp_ct FROM records")?;
    let stored = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, RecordId>("record_id")?,
                row.get::<_, Option<Vec<u8>>>("meta_ct")?,
                row.get::<_, Option<Vec<u8>>>("password_ct")?,
                row.get::<_, Option<Vec<u8>>>("notes_ct")?,
                row.get::<_, Option<Vec<u8>>>("totp_ct")?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    for (record_id, meta_ct, password_ct, notes_ct, totp_ct) in stored {
        let fields = metadata::open(from, Place::Record, &record_id, meta_ct.as_deref())?;
        let meta_ct = metadata::seal(to, Place::Record, &record_id, &fields)?;

        let mut resealed: Vec<Option<Vec<u8>>> = Vec::with_capacity(3);
        for (field, blob) in [
            (SecretField::Password, password_ct),
            (SecretField::Notes, notes_ct),
            (SecretField::TotpSecret, totp_ct),
        ] {
            let plaintext = open_field(from, &record_id, field, &blob)?;
            resealed.push(seal_field(to, &record_id, field, plaintext.as_deref())?);
        }

        tx.execute(
            "UPDATE records SET meta_ct = ?2, password_ct = ?3, notes_ct = ?4, totp_ct = ?5
             WHERE record_id = ?1",
            rusqlite::params![record_id, meta_ct, resealed[0], resealed[1], resealed[2]],
        )?;
    }

    Ok(())
}

pub(crate) fn seal_field(
    key: &VaultKey,
    record_id: &str,
    field: SecretField,
    value: Option<&str>,
) -> CoreResult<Option<Vec<u8>>> {
    match value {
        // Незаполненный секрет — это NULL, а не шифротекст пустой строки: иначе
        // `has_notes` перестал бы совпадать с «заметка есть» (`mock/seed.ts:22`).
        None => Ok(None),
        Some(text) => {
            let aad = crypto::field_aad(record_id, field.as_str());
            crypto::seal(key, &aad, text.as_bytes()).map(Some)
        }
    }
}

pub(crate) fn open_field(
    key: &VaultKey,
    record_id: &str,
    field: SecretField,
    blob: &Option<Vec<u8>>,
) -> CoreResult<Option<String>> {
    let Some(blob) = blob else { return Ok(None) };

    let aad = crypto::field_aad(record_id, field.as_str());
    let plaintext = crypto::open(key, &aad, blob)?;
    let text = String::from_utf8(plaintext)
        .map_err(|_| CoreError::internal("Зашифрованное значение повреждено."))?;
    Ok(Some(text))
}
