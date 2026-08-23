//! Расхождения версий: обнаружение, хранение и разрешение (F11, §5.5).
//!
//! Конфликт заводится **только** обменом и без него не существует — поэтому
//! модуль лежит внутри [`crate::sync`], а не рядом с ним. Наверху ([`super`])
//! живёт правило «чья версия свежее», здесь — что делать, когда однозначного
//! ответа у этого правила нет.
//!
//! # Три вещи, из которых следует всё остальное
//!
//! **Конфликт — это факт о ПРЕДКЕ, а не о числах.** «Версия 5 против версии 5»
//! само по себе не значит ничего: обе стороны могли прийти к пятёрке одной и той
//! же правкой. Расхождение начинается там, где обе стороны ушли от общей точки —
//! от `sync_state.synced_version`. Нет такой точки (свежее сопряжение, возврат
//! после отзыва) — нет и расхождения: см. [`super::classify`].
//!
//! **Приехавшая версия хранится целиком, включая секреты.** Больше её девать
//! некуда: в `records` она легла бы поверх местной, то есть ровно тем тихим
//! затиранием, от которого конфликт и защищает. Секреты при этом перешифрованы
//! местным ключом (шифротексты сторон несовместимы, см. шапку [`super`]) и под
//! **своим** AAD — [`crypto::conflict_field_aad`].
//!
//! **Местная сторона в списке — живая запись, а не снимок.** Пока конфликт ждёт
//! решения, запись можно править, и показывать человеку устаревший вариант его
//! же данных было бы враньём. Замороженная в строке величина ровно одна —
//! `raised_at`.

use std::collections::HashSet;
use std::fmt;

use rusqlite::{Connection, OptionalExtension, Row};

use crate::crypto::{self, VaultKey};
use crate::error::{CoreError, CoreResult};
use crate::model::{
    now_iso, ConflictField, ConflictSide, ConflictVersion, IsoDateTime, RecordConflict, RecordId,
    SecretField, VaultId,
};
use crate::storage::records;
use crate::trust;

use super::SyncRecord;

/// Чем подписывается сторона, чьё устройство в `devices` не нашлось.
///
/// Строка отозванного устройства не удаляется (§2.3), так что случай почти
/// невозможен — но «конфликт неизвестно с кем» человеку показать всё равно
/// честнее, чем уронить весь список из-за подписи под колонкой.
const UNKNOWN_PEER: &str = "Другое устройство";

const COLUMNS: &str = "record_id, device_id, raised_at, local_version, version,
                       vault_id, service_name, urls, login, account_label,
                       password_ct, notes_ct, totp_ct, updated_at, password_updated_at";

// ---------------------------------------------------------------------------
// Строка таблицы
// ---------------------------------------------------------------------------

/// Приехавшая версия, ждущая решения. Секреты — нераспечатанными: список
/// конфликтов их не показывает, и открывать их ради него незачем.
pub struct StoredConflict {
    pub record_id: RecordId,
    pub device_id: String,
    pub raised_at: IsoDateTime,
    /// Местная версия на момент подъёма — только для того, чтобы не поднимать
    /// одно и то же расхождение событием на каждом круге.
    pub local_version: i64,
    pub version: i64,
    pub vault_id: VaultId,
    pub service_name: String,
    pub urls: Vec<String>,
    pub login: String,
    pub account_label: Option<String>,
    password_ct: Option<Vec<u8>>,
    notes_ct: Option<Vec<u8>>,
    totp_ct: Option<Vec<u8>>,
    pub updated_at: IsoDateTime,
    pub password_updated_at: IsoDateTime,
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<StoredConflict> {
    let urls: String = row.get("urls")?;
    Ok(StoredConflict {
        record_id: row.get("record_id")?,
        device_id: row.get("device_id")?,
        raised_at: row.get("raised_at")?,
        local_version: row.get("local_version")?,
        version: row.get("version")?,
        vault_id: row.get("vault_id")?,
        service_name: row.get("service_name")?,
        // Список пришёл из нашей же схемы; битый JSON здесь — испорченный файл,
        // и показать версию без доменов лучше, чем уронить экран разрешения.
        urls: serde_json::from_str(&urls).unwrap_or_default(),
        login: row.get("login")?,
        account_label: row.get("account_label")?,
        password_ct: row.get("password_ct")?,
        notes_ct: row.get("notes_ct")?,
        totp_ct: row.get("totp_ct")?,
        updated_at: row.get("updated_at")?,
        password_updated_at: row.get("password_updated_at")?,
    })
}

impl StoredConflict {
    /// Распечатать приехавшие секреты. Ключ местный, AAD — конфликтный.
    pub fn secrets(&self, key: &VaultKey) -> CoreResult<Secrets> {
        Ok(Secrets {
            password: open(
                key,
                &self.record_id,
                SecretField::Password,
                &self.password_ct,
            )?,
            notes: open(key, &self.record_id, SecretField::Notes, &self.notes_ct)?,
            totp_secret: open(key, &self.record_id, SecretField::TotpSecret, &self.totp_ct)?,
        })
    }

    fn fields(&self) -> Fields<'_> {
        Fields {
            vault_id: &self.vault_id,
            service_name: &self.service_name,
            urls: &self.urls,
            login: &self.login,
            account_label: self.account_label.as_deref(),
        }
    }

    fn version_view(&self, device_name: String) -> ConflictVersion {
        ConflictVersion {
            side: ConflictSide::Remote,
            device_name,
            version: self.version,
            updated_at: self.updated_at.clone(),
            vault_id: self.vault_id.clone(),
            service_name: self.service_name.clone(),
            urls: self.urls.clone(),
            login: self.login.clone(),
            account_label: self.account_label.clone(),
            // По наличию шифротекста, как и у `RecordMeta`: незаполненный секрет
            // в хранилище — это NULL, а не шифротекст пустой строки.
            has_notes: self.notes_ct.is_some(),
            has_totp: self.totp_ct.is_some(),
        }
    }
}

/// Три секретных поля одной стороны.
///
/// `password` тоже необязателен, хотя в `RecordSecrets` он обязателен: у
/// приехавшего надгробия секретов нет вовсе.
#[derive(Default, PartialEq, Eq)]
pub struct Secrets {
    pub password: Option<String>,
    pub notes: Option<String>,
    pub totp_secret: Option<String>,
}

impl Secrets {
    pub fn field(&self, field: SecretField) -> Option<String> {
        match field {
            SecretField::Password => self.password.clone(),
            SecretField::Notes => self.notes.clone(),
            SecretField::TotpSecret => self.totp_secret.clone(),
        }
    }
}

/// `Debug` руками — по той же причине, что у [`super::SyncRecord`]: производный
/// вывалил бы пароль в первый же лог.
impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secrets").finish_non_exhaustive()
    }
}

fn seal(
    key: &VaultKey,
    record_id: &str,
    field: SecretField,
    value: Option<&str>,
) -> CoreResult<Option<Vec<u8>>> {
    match value {
        None => Ok(None),
        Some(text) => {
            let aad = crypto::conflict_field_aad(record_id, field.as_str());
            crypto::seal(key, &aad, text.as_bytes()).map(Some)
        }
    }
}

fn open(
    key: &VaultKey,
    record_id: &str,
    field: SecretField,
    blob: &Option<Vec<u8>>,
) -> CoreResult<Option<String>> {
    let Some(blob) = blob else { return Ok(None) };

    let aad = crypto::conflict_field_aad(record_id, field.as_str());
    let plaintext = crypto::open(key, &aad, blob)?;
    let text = String::from_utf8(plaintext)
        .map_err(|_| CoreError::internal("Зашифрованное значение повреждено."))?;
    Ok(Some(text))
}

// ---------------------------------------------------------------------------
// Чтение и запись
// ---------------------------------------------------------------------------

pub fn open_for(conn: &Connection, record_id: &str) -> CoreResult<Option<StoredConflict>> {
    let sql = format!("SELECT {COLUMNS} FROM conflicts WHERE record_id = ?1");
    Ok(conn.query_row(&sql, [record_id], from_row).optional()?)
}

/// Записи, по которым спор открыт.
///
/// Нужны обмену: объявить такую запись «сошедшейся» значило бы подвинуть предка
/// и потерять расхождение из виду навсегда (см. [`super::plan`]).
pub fn open_ids(conn: &Connection) -> CoreResult<HashSet<RecordId>> {
    let mut stmt = conn.prepare("SELECT record_id FROM conflicts")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    Ok(ids)
}

/// Отложить приехавшую версию как спорную.
///
/// `true` — расхождение новое или изменившееся, и о нём стоит рассказать
/// событием. Пара «местная версия ‖ приехавшая версия» повторилась — значит, это
/// тот же спор, который уже висит на экране, и второе `conflict_raised` про него
/// было бы шумом: круги идут каждую минуту, а конфликт ждёт сколько нужно.
pub fn raise(
    conn: &Connection,
    key: &VaultKey,
    device_id: &str,
    incoming: &SyncRecord,
    local_version: i64,
) -> CoreResult<bool> {
    let known: Option<(i64, i64)> = conn
        .query_row(
            "SELECT local_version, version FROM conflicts WHERE record_id = ?1",
            [&incoming.record_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let fresh = known != Some((local_version, incoming.version));

    let record_id = &incoming.record_id;
    conn.execute(
        "INSERT INTO conflicts (record_id, device_id, raised_at, local_version, version,
                                vault_id, service_name, urls, login, account_label,
                                password_ct, notes_ct, totp_ct, updated_at, password_updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(record_id) DO UPDATE SET
             device_id           = excluded.device_id,
             raised_at           = excluded.raised_at,
             local_version       = excluded.local_version,
             version             = excluded.version,
             vault_id            = excluded.vault_id,
             service_name        = excluded.service_name,
             urls                = excluded.urls,
             login               = excluded.login,
             account_label       = excluded.account_label,
             password_ct         = excluded.password_ct,
             notes_ct            = excluded.notes_ct,
             totp_ct             = excluded.totp_ct,
             updated_at          = excluded.updated_at,
             password_updated_at = excluded.password_updated_at",
        rusqlite::params![
            record_id,
            device_id,
            now_iso(),
            local_version,
            incoming.version,
            incoming.vault_id,
            incoming.service_name,
            serde_json::to_string(&incoming.urls)?,
            incoming.login,
            incoming.account_label,
            seal(
                key,
                record_id,
                SecretField::Password,
                incoming.password.as_deref()
            )?,
            seal(
                key,
                record_id,
                SecretField::Notes,
                incoming.notes.as_deref()
            )?,
            seal(
                key,
                record_id,
                SecretField::TotpSecret,
                incoming.totp_secret.as_deref()
            )?,
            incoming.updated_at,
            incoming.password_updated_at,
        ],
    )?;

    Ok(fresh)
}

/// Спорить больше не о чем: запись переехала, её удалили или человек выбрал.
pub fn drop_for(conn: &Connection, record_id: &str) -> CoreResult<()> {
    conn.execute("DELETE FROM conflicts WHERE record_id = ?1", [record_id])?;
    Ok(())
}

/// Перешифровать отложенные версии с одного ключа хранилища на другой.
///
/// Близнец [`records::rekey_all`], и звать его надо в той же транзакции:
/// пропустить эту таблицу — значит потерять приехавшую версию при первой же
/// смене мастер-пароля и узнать об этом только на экране разрешения. AAD
/// прежний: `record_id` и имена полей не меняются.
pub fn rekey_all(tx: &rusqlite::Transaction<'_>, from: &VaultKey, to: &VaultKey) -> CoreResult<()> {
    let mut stmt = tx.prepare(
        "SELECT record_id, password_ct, notes_ct, totp_ct FROM conflicts
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
            let plaintext = open(from, &record_id, field, &blob)?;
            resealed.push(seal(to, &record_id, field, plaintext.as_deref())?);
        }

        tx.execute(
            "UPDATE conflicts SET password_ct = ?2, notes_ct = ?3, totp_ct = ?4
             WHERE record_id = ?1",
            rusqlite::params![record_id, resealed[0], resealed[1], resealed[2]],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Сравнение сторон
// ---------------------------------------------------------------------------

/// Метаданные, по которым версии могут разойтись.
///
/// Отдельный тип, чтобы одно и то же сравнение работало и для строки `records`,
/// и для строки `conflicts`, и для записи с провода: разъехаться этим трём
/// нельзя — расхождение здесь означало бы конфликт, который видно с одной
/// стороны и не видно с другой.
pub struct Fields<'a> {
    pub vault_id: &'a str,
    pub service_name: &'a str,
    pub urls: &'a [String],
    pub login: &'a str,
    pub account_label: Option<&'a str>,
}

/// `None` и пустая строка — одно и то же «поле не заполнено»
/// (`mock/conflicts.ts::sameText`). Без trim: пробел по краям пароля —
/// легитимный символ, и «различаются» здесь должно значить «различаются».
fn same_text(a: Option<&str>, b: Option<&str>) -> bool {
    a.unwrap_or_default() == b.unwrap_or_default()
}

/// По каким полям версии расходятся.
///
/// ЗАКОН №1: секреты сравниваются ЗДЕСЬ, внутри ядра, и наружу уходит только имя
/// поля. «Пароли различаются» — факт о записи; сам пароль для этого факта UI не
/// нужен и не выдаётся.
///
/// Порядок перечисления фиксирован и совпадает с
/// `mock/conflicts.ts::conflictDiff`: фронт сверяет массив целиком, а не как
/// множество.
pub fn differing_fields(
    local: &Fields<'_>,
    local_secrets: &Secrets,
    remote: &Fields<'_>,
    remote_secrets: &Secrets,
) -> Vec<ConflictField> {
    let mut fields = Vec::new();

    if local.vault_id != remote.vault_id {
        fields.push(ConflictField::VaultId);
    }
    if local.service_name != remote.service_name {
        fields.push(ConflictField::ServiceName);
    }
    // Последовательность, а не множество: порядок доменов человек задавал сам.
    if local.urls != remote.urls {
        fields.push(ConflictField::Urls);
    }
    if local.login != remote.login {
        fields.push(ConflictField::Login);
    }
    if !same_text(local.account_label, remote.account_label) {
        fields.push(ConflictField::AccountLabel);
    }
    if !same_text(
        local_secrets.password.as_deref(),
        remote_secrets.password.as_deref(),
    ) {
        fields.push(ConflictField::Password);
    }
    if !same_text(
        local_secrets.notes.as_deref(),
        remote_secrets.notes.as_deref(),
    ) {
        fields.push(ConflictField::Notes);
    }
    if !same_text(
        local_secrets.totp_secret.as_deref(),
        remote_secrets.totp_secret.as_deref(),
    ) {
        fields.push(ConflictField::TotpSecret);
    }

    fields
}

// ---------------------------------------------------------------------------
// Живая сторона
// ---------------------------------------------------------------------------

/// Местная сторона: живая запись плюс её распечатанные секреты.
pub struct LocalSide {
    pub vault_id: VaultId,
    pub service_name: String,
    pub urls: Vec<String>,
    pub login: String,
    pub account_label: Option<String>,
    pub version: i64,
    pub updated_at: IsoDateTime,
    pub password_updated_at: IsoDateTime,
    pub has_notes: bool,
    pub has_totp: bool,
    pub secrets: Secrets,
}

impl LocalSide {
    pub fn fields(&self) -> Fields<'_> {
        Fields {
            vault_id: &self.vault_id,
            service_name: &self.service_name,
            urls: &self.urls,
            login: &self.login,
            account_label: self.account_label.as_deref(),
        }
    }
}

/// Живая запись как сторона конфликта.
///
/// `None` — записи нет или она стала надгробием: спорить о версиях удалённой
/// записи не о чем.
pub fn local_side(
    conn: &Connection,
    key: &VaultKey,
    record_id: &str,
) -> CoreResult<Option<LocalSide>> {
    let row = conn
        .query_row(
            "SELECT vault_id, service_name, urls, login, account_label,
                    password_ct, notes_ct, totp_ct, version, updated_at, password_updated_at
             FROM records WHERE record_id = ?1 AND deleted_at IS NULL",
            [record_id],
            |row| {
                let urls: String = row.get("urls")?;
                Ok(RawLocal {
                    vault_id: row.get("vault_id")?,
                    service_name: row.get("service_name")?,
                    urls: serde_json::from_str(&urls).unwrap_or_default(),
                    login: row.get("login")?,
                    account_label: row.get("account_label")?,
                    password_ct: row.get("password_ct")?,
                    notes_ct: row.get("notes_ct")?,
                    totp_ct: row.get("totp_ct")?,
                    version: row.get("version")?,
                    updated_at: row.get("updated_at")?,
                    password_updated_at: row.get("password_updated_at")?,
                })
            },
        )
        .optional()?;

    let Some(raw) = row else { return Ok(None) };

    // Секреты живой записи лежат под ОБЫЧНЫМ AAD: это запись, а не отложенная
    // версия, и распечатывать её надо тем же ключом, что и всегда.
    let secrets = Secrets {
        password: records::open_field(key, record_id, SecretField::Password, &raw.password_ct)?,
        notes: records::open_field(key, record_id, SecretField::Notes, &raw.notes_ct)?,
        totp_secret: records::open_field(key, record_id, SecretField::TotpSecret, &raw.totp_ct)?,
    };

    Ok(Some(LocalSide {
        vault_id: raw.vault_id,
        service_name: raw.service_name,
        urls: raw.urls,
        login: raw.login,
        account_label: raw.account_label,
        version: raw.version,
        updated_at: raw.updated_at,
        password_updated_at: raw.password_updated_at,
        has_notes: raw.notes_ct.is_some(),
        has_totp: raw.totp_ct.is_some(),
        secrets,
    }))
}

/// Строка `records` до расшифровки. Отдельный тип, чтобы одиннадцать полей не
/// разъезжались по индексам кортежа — тот же приём, что у `StoredForSync`.
struct RawLocal {
    vault_id: VaultId,
    service_name: String,
    urls: Vec<String>,
    login: String,
    account_label: Option<String>,
    password_ct: Option<Vec<u8>>,
    notes_ct: Option<Vec<u8>>,
    totp_ct: Option<Vec<u8>>,
    version: i64,
    updated_at: IsoDateTime,
    password_updated_at: IsoDateTime,
}

// ---------------------------------------------------------------------------
// Список для UI
// ---------------------------------------------------------------------------

/// Собрать один конфликт. `None` — конфликта нет либо запись исчезла.
pub fn build(
    conn: &Connection,
    key: &VaultKey,
    this_device_id: &str,
    record_id: &str,
) -> CoreResult<Option<RecordConflict>> {
    let Some(stored) = open_for(conn, record_id)? else {
        return Ok(None);
    };
    let Some(local) = local_side(conn, key, record_id)? else {
        return Ok(None);
    };
    Ok(Some(assemble(conn, key, this_device_id, &stored, &local)?))
}

fn assemble(
    conn: &Connection,
    key: &VaultKey,
    this_device_id: &str,
    stored: &StoredConflict,
    local: &LocalSide,
) -> CoreResult<RecordConflict> {
    let remote_secrets = stored.secrets(key)?;

    Ok(RecordConflict {
        record_id: stored.record_id.clone(),
        raised_at: stored.raised_at.clone(),
        local: ConflictVersion {
            side: ConflictSide::Local,
            device_name: device_name(conn, this_device_id)?,
            version: local.version,
            updated_at: local.updated_at.clone(),
            vault_id: local.vault_id.clone(),
            service_name: local.service_name.clone(),
            urls: local.urls.clone(),
            login: local.login.clone(),
            account_label: local.account_label.clone(),
            has_notes: local.has_notes,
            has_totp: local.has_totp,
        },
        remote: stored.version_view(device_name(conn, &stored.device_id)?),
        differing_fields: differing_fields(
            &local.fields(),
            &local.secrets,
            &stored.fields(),
            &remote_secrets,
        ),
    })
}

fn device_name(conn: &Connection, device_id: &str) -> CoreResult<String> {
    Ok(trust::name_of(conn, device_id)?.unwrap_or_else(|| UNKNOWN_PEER.to_owned()))
}

/// Все ждущие расхождения.
///
/// Конфликты, чья запись исчезла или стала надгробием, из списка молча выпадают:
/// это не ошибка, а нормальный исход — человек удалил запись, о версиях которой
/// шёл спор (`mock/index.ts:1345`).
pub fn list(
    conn: &Connection,
    key: &VaultKey,
    this_device_id: &str,
) -> CoreResult<Vec<RecordConflict>> {
    let sql = format!("SELECT {COLUMNS} FROM conflicts ORDER BY raised_at, record_id");
    let mut stmt = conn.prepare(&sql)?;
    let stored = stmt
        .query_map([], from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let mut out = Vec::with_capacity(stored.len());
    for entry in stored {
        let Some(local) = local_side(conn, key, &entry.record_id)? else {
            continue;
        };
        out.push(assemble(conn, key, this_device_id, &entry, &local)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side<'a>(vault: &'a str, urls: &'a [String], label: Option<&'a str>) -> Fields<'a> {
        Fields {
            vault_id: vault,
            service_name: "GitHub",
            urls,
            login: "octocat",
            account_label: label,
        }
    }

    fn secrets(password: &str, notes: Option<&str>) -> Secrets {
        Secrets {
            password: Some(password.to_owned()),
            notes: notes.map(str::to_owned),
            totp_secret: None,
        }
    }

    #[test]
    fn identical_sides_differ_in_nothing() {
        let urls = vec!["github.com".to_owned()];
        let diff = differing_fields(
            &side("v1", &urls, None),
            &secrets("тайна", None),
            &side("v1", &urls, None),
            &secrets("тайна", None),
        );
        assert!(diff.is_empty(), "{diff:?}");
    }

    #[test]
    fn fields_are_listed_in_the_order_the_contract_expects() {
        let mine = vec!["github.com".to_owned()];
        let theirs = vec!["github.com".to_owned(), "gist.github.com".to_owned()];
        let diff = differing_fields(
            &side("v1", &mine, None),
            &secrets("тайна-1", Some("заметка")),
            &side("v2", &theirs, Some("Рабочий")),
            &secrets("тайна-2", None),
        );

        assert_eq!(
            diff,
            vec![
                ConflictField::VaultId,
                ConflictField::Urls,
                ConflictField::AccountLabel,
                ConflictField::Password,
                ConflictField::Notes,
            ]
        );
    }

    #[test]
    fn an_empty_optional_equals_an_absent_one() {
        let urls = vec!["github.com".to_owned()];
        // `null` и пустая строка — одно и то же «поле не заполнено».
        let diff = differing_fields(
            &side("v1", &urls, Some("")),
            &secrets("тайна", Some("")),
            &side("v1", &urls, None),
            &secrets("тайна", None),
        );
        assert!(diff.is_empty(), "{diff:?}");
    }

    #[test]
    fn the_order_of_urls_is_a_difference() {
        let mine = vec!["a.com".to_owned(), "b.com".to_owned()];
        let theirs = vec!["b.com".to_owned(), "a.com".to_owned()];
        let diff = differing_fields(
            &side("v1", &mine, None),
            &secrets("тайна", None),
            &side("v1", &theirs, None),
            &secrets("тайна", None),
        );
        assert_eq!(diff, vec![ConflictField::Urls]);
    }
}
