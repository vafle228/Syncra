//! Записи паролей (§4.1, F4/F5).
//!
//! Единица данных всей системы. Метаданные лежат в БД открыто, секреты — только
//! шифротекстом, и разделение проведено по колонкам: [`list`] физически не может
//! вернуть секрет, потому что не читает и не расшифровывает ни одного BLOB.

use rusqlite::{Connection, OptionalExtension, Row};

use crate::crypto::{self, VaultKey};
use crate::error::{CoreError, CoreResult};
use crate::model::{
    normalize_optional_secret, now_iso, require_non_empty, require_present, IsoDateTime,
    RecordDraft, RecordId, RecordMeta, RecordPatch, RecordSecrets, SecretField,
};

/// Запись как она лежит в БД: метаданные плюс нераспечатанные секреты.
struct StoredRecord {
    meta: RecordMeta,
    password_ct: Option<Vec<u8>>,
    notes_ct: Option<Vec<u8>>,
    totp_ct: Option<Vec<u8>>,
}

const COLUMNS: &str = "record_id, vault_id, service_name, urls, login, account_label,
                       password_ct, notes_ct, totp_ct,
                       version, created_at, updated_at, password_updated_at, deleted_at";

fn from_row(row: &Row<'_>) -> rusqlite::Result<StoredRecord> {
    let urls_json: String = row.get("urls")?;
    let password_ct: Option<Vec<u8>> = row.get("password_ct")?;
    let notes_ct: Option<Vec<u8>> = row.get("notes_ct")?;
    let totp_ct: Option<Vec<u8>> = row.get("totp_ct")?;

    Ok(StoredRecord {
        meta: RecordMeta {
            record_id: row.get("record_id")?,
            vault_id: row.get("vault_id")?,
            service_name: row.get("service_name")?,
            // Список пришёл из нашей же схемы; битый JSON здесь — испорченный файл,
            // а не пользовательский ввод, и разумнее показать запись без доменов,
            // чем уронить весь список.
            urls: serde_json::from_str(&urls_json).unwrap_or_default(),
            login: row.get("login")?,
            account_label: row.get("account_label")?,
            // Флаги — по наличию шифротекста. Ни одного `open()` ради рамки в карточке.
            has_notes: notes_ct.is_some(),
            has_totp: totp_ct.is_some(),
            version: row.get("version")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            password_updated_at: row.get("password_updated_at")?,
            deleted_at: row.get("deleted_at")?,
        },
        password_ct,
        notes_ct,
        totp_ct,
    })
}

fn fetch(conn: &Connection, record_id: &str) -> CoreResult<Option<StoredRecord>> {
    let sql = format!("SELECT {COLUMNS} FROM records WHERE record_id = ?1");
    Ok(conn.query_row(&sql, [record_id], from_row).optional()?)
}

/// Живая запись или `NOT_FOUND`. Надгробие — тоже «не найдено»: править и
/// открывать удалённую запись нечего (§5.4).
fn fetch_live(conn: &Connection, record_id: &str) -> CoreResult<StoredRecord> {
    match fetch(conn, record_id)? {
        Some(stored) if stored.meta.deleted_at.is_none() => Ok(stored),
        _ => Err(CoreError::not_found("Запись не найдена.")),
    }
}

pub fn list(
    conn: &Connection,
    vault_id: Option<&str>,
    include_deleted: bool,
) -> CoreResult<Vec<RecordMeta>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM records
         WHERE (?1 IS NULL OR vault_id = ?1)
           AND (?2 = 1 OR deleted_at IS NULL)"
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut records = stmt
        .query_map(rusqlite::params![vault_id, include_deleted as i64], |row| {
            from_row(row).map(|stored| stored.meta)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Порядок ядра — по сервису, затем по логину, без учёта регистра. Стор фронта
    // всё равно пересортировывает у себя через `localeCompare`, поэтому побайтовая
    // параль с JS не нужна; нужен предсказуемый порядок.
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
    let service_name = require_non_empty(&draft.service_name, "Сервис")?;
    let login = require_non_empty(&draft.login, "Логин")?;
    let password = require_present(&draft.password, "Пароль")?;

    // ID генерирует ядро, а не UI (§4.1): это ключ синхронизации и разрешения
    // конфликтов, и приходить снаружи он не может.
    let record_id: RecordId = uuid::Uuid::new_v4().to_string();
    let created_at: IsoDateTime = now_iso();

    let password_ct = seal_field(key, &record_id, SecretField::Password, Some(&password))?;
    let notes_ct = seal_field(
        key,
        &record_id,
        SecretField::Notes,
        normalize_optional_secret(draft.notes.as_deref()).as_deref(),
    )?;
    let totp_ct = seal_field(
        key,
        &record_id,
        SecretField::TotpSecret,
        normalize_optional_secret(draft.totp_secret.as_deref()).as_deref(),
    )?;

    conn.execute(
        "INSERT INTO records (record_id, vault_id, service_name, urls, login, account_label,
                              password_ct, notes_ct, totp_ct,
                              version, created_at, updated_at, password_updated_at, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?10, ?10, NULL)",
        rusqlite::params![
            record_id,
            vault_id,
            service_name,
            serde_json::to_string(&draft.urls)?,
            login,
            draft.account_label.as_deref(),
            password_ct,
            notes_ct,
            totp_ct,
            created_at,
        ],
    )?;

    Ok(fetch_live(conn, &record_id)?.meta)
}

/// `vault_id` в патче приходит уже проверенным — как и при создании.
pub fn update(
    conn: &Connection,
    key: &VaultKey,
    record_id: &str,
    patch: &RecordPatch,
) -> CoreResult<RecordMeta> {
    let current = fetch_live(conn, record_id)?;
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
            normalize_optional_secret(value.as_deref()).as_deref(),
        )?,
    };
    let totp_ct = match patch.totp_secret.as_ref() {
        None => current.totp_ct.clone(),
        Some(value) => seal_field(
            key,
            record_id,
            SecretField::TotpSecret,
            normalize_optional_secret(value.as_deref()).as_deref(),
        )?,
    };

    let service_name = match patch.service_name.as_deref() {
        Some(value) => require_non_empty(value, "Сервис")?,
        None => current.meta.service_name.clone(),
    };
    let login = match patch.login.as_deref() {
        Some(value) => require_non_empty(value, "Логин")?,
        None => current.meta.login.clone(),
    };
    let urls = patch
        .urls
        .clone()
        .unwrap_or_else(|| current.meta.urls.clone());
    let account_label = match patch.account_label.as_ref() {
        Some(value) => value.clone(),
        None => current.meta.account_label.clone(),
    };
    let vault_id = patch
        .vault_id
        .clone()
        .unwrap_or_else(|| current.meta.vault_id.clone());

    conn.execute(
        "UPDATE records SET
             vault_id = ?2, service_name = ?3, urls = ?4, login = ?5, account_label = ?6,
             password_ct = ?7, notes_ct = ?8, totp_ct = ?9,
             version = version + 1, updated_at = ?10, password_updated_at = ?11
         WHERE record_id = ?1",
        rusqlite::params![
            record_id,
            vault_id,
            service_name,
            serde_json::to_string(&urls)?,
            login,
            account_label,
            password_ct,
            notes_ct,
            totp_ct,
            changed_at,
            if password_changed {
                &changed_at
            } else {
                &current.meta.password_updated_at
            },
        ],
    )?;

    Ok(fetch_live(conn, record_id)?.meta)
}

/// Мягкое удаление (§5.4).
///
/// Надгробие хранит `record_id`, `deleted_at` и `version` — и больше ничего:
/// секреты стираются здесь же, а не ждут чистки надгробий через 30 дней.
/// Вместе с ними перестают быть правдой `has_notes` / `has_totp`, и они
/// обнуляются сами — по тому же правилу «флаг = наличие шифротекста».
pub fn delete(conn: &Connection, record_id: &str) -> CoreResult<RecordMeta> {
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
        Some(stored) => Ok(stored.meta),
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

/// Перешифровать все секреты хранилища с одного ключа на другой (F13, смена
/// мастер-пароля).
///
/// Метаданные записей не трогаются вовсе: `version`, `updated_at` и
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
    let mut stmt = tx.prepare(
        "SELECT record_id, password_ct, notes_ct, totp_ct FROM records
         WHERE password_ct IS NOT NULL OR notes_ct IS NOT NULL OR totp_ct IS NOT NULL",
    )?;
    let stored = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("record_id")?,
                row.get::<_, Option<Vec<u8>>>("password_ct")?,
                row.get::<_, Option<Vec<u8>>>("notes_ct")?,
                row.get::<_, Option<Vec<u8>>>("totp_ct")?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    for (record_id, password_ct, notes_ct, totp_ct) in stored {
        let fields = [
            (SecretField::Password, password_ct),
            (SecretField::Notes, notes_ct),
            (SecretField::TotpSecret, totp_ct),
        ];

        let mut resealed: Vec<Option<Vec<u8>>> = Vec::with_capacity(fields.len());
        for (field, blob) in fields {
            let plaintext = open_field(from, &record_id, field, &blob)?;
            resealed.push(seal_field(to, &record_id, field, plaintext.as_deref())?);
        }

        tx.execute(
            "UPDATE records SET password_ct = ?2, notes_ct = ?3, totp_ct = ?4
             WHERE record_id = ?1",
            rusqlite::params![record_id, resealed[0], resealed[1], resealed[2]],
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
