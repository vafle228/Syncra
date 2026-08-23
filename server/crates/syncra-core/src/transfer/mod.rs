//! Перенос данных: CSV, зашифрованный бэкап и импорт (§8.3 «Import/Export»).
//!
//! Главное решение всего F12 стоит выше кода: **ни CSV, ни бэкап не пересекают
//! границу IPC**. Файл целиком собирает и пишет ядро, наружу уходит только
//! [`ExportFile`] — путь, размер, число записей. Иначе экспорт стал бы
//! четвёртой командой Закона №1, причём отдающей не одно поле по нажатию, а ВСЕ
//! пароли разом, одной строкой в куче JS.
//!
//! Импорт устроен зеркально: разобранные чужие пароли остаются в [`Session`]
//! внутри ядра до `commit_import`, наружу идёт предпросмотр без единого пароля.
//!
//! **Про `?` на файловых операциях.** `error.rs` переводит любую
//! `std::io::Error` в «Не удалось связаться с устройством в сети» — это правда
//! для сетевого слоя, ради которого impl и написан, и ложь здесь. Поэтому в
//! этом модуле каждая файловая ошибка отображается руками, через [`io_failed`].

pub mod backup;
pub mod csv;
pub mod import;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Utc};
use rusqlite::Connection;

use crate::crypto::VaultKey;
use crate::error::{CoreError, CoreResult};
use crate::model::{
    now_iso, ExportFile, ImportSource, RecordMeta, SecretField, VAULT_NAME_MAX_LENGTH,
};
use crate::storage::{metadata, records, vaults};

use backup::{Backup, BackupRecord, BackupVault};
use import::ImportEntry;

/// Потолок читаемого файла импорта.
///
/// Чужой файл уезжает в память целиком — иначе не посчитать статусы строк, — и
/// без потолка «выберите файл» становится способом положить процесс, выбрав
/// образ диска. Тридцать два мегабайта это порядка ста тысяч паролей: столько
/// не бывает даже у корпоративного экспорта.
pub const IMPORT_FILE_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Цвет секции, которую ядро заводит под импорт (`mock/index.ts:706`).
const IMPORT_VAULT_COLOR: &str = "mint";

/// Разобранный файл, ждущий согласия человека.
///
/// В файл не попадает и замок не переживает: здесь лежат ЧУЖИЕ пароли открытым
/// текстом, и закрытое хранилище — не то место, где им можно продолжать лежать.
#[derive(Debug)]
pub struct Session {
    pub source: ImportSource,
    pub file_name: String,
    /// Откуда файл прочитан: `commit_import` обещает его удалить (§3.10 макета).
    pub path: PathBuf,
    pub entries: Vec<ImportEntry>,
}

// ---------------------------------------------------------------------------
// Файловые операции
// ---------------------------------------------------------------------------

/// Единственный способ превратить `std::io::Error` в ошибку ядра в этом модуле.
/// Ни пути, ни системного текста наружу: `message` уходит прямо в UI.
fn io_failed(message: &'static str) -> impl Fn(std::io::Error) -> CoreError {
    move |_| CoreError::internal(message)
}

/// Записать файл экспорта и вернуть его след.
///
/// Повторный экспорт в тот же день ПЕРЕЗАПИСЫВАЕТ файл, а не плодит второй
/// (`mock/index.ts:678`): человек делает бэкап дважды, потому что в первый раз
/// не понял, где он оказался, — а не потому, что хочет две копии.
pub fn write_export(
    dir: &Path,
    file_name: &str,
    bytes: &[u8],
    encrypted: bool,
    record_count: i64,
) -> CoreResult<ExportFile> {
    std::fs::create_dir_all(dir)
        .map_err(io_failed("Не удалось открыть папку для сохранения файла."))?;

    let path = dir.join(file_name);
    std::fs::write(&path, bytes).map_err(io_failed("Не удалось записать файл на диск."))?;
    // Права ужимаются по той же причине, что у самого хранилища: в CSV лежат все
    // пароли открытым текстом, и читаемым для всех он быть не должен.
    crate::storage::restrict(&path, 0o600);

    Ok(ExportFile {
        path: path.to_string_lossy().into_owned(),
        file_name: file_name.to_owned(),
        size_bytes: bytes.len() as i64,
        record_count,
        created_at: now_iso(),
        encrypted,
    })
}

/// Прочитать файл импорта: сначала размер, потом содержимое.
///
/// Не UTF-8 — это `VALIDATION`, а не внутренняя ошибка: человек выбрал не тот
/// файл, и сказать ему надо именно это.
pub fn read_import_file(path: &Path) -> CoreResult<String> {
    let size = std::fs::metadata(path)
        .map_err(io_failed("Не удалось открыть выбранный файл."))?
        .len();
    if size > IMPORT_FILE_MAX_BYTES {
        return Err(CoreError::validation(format!(
            "Файл больше {} МБ — это не экспорт паролей.",
            IMPORT_FILE_MAX_BYTES / (1024 * 1024)
        )));
    }

    let bytes = std::fs::read(path).map_err(io_failed("Не удалось прочитать выбранный файл."))?;
    String::from_utf8(bytes).map_err(|_| {
        CoreError::validation("Файл не читается как текст. Проверьте, что это экспорт паролей.")
    })
}

/// Прочитать файл бэкапа — байтами, а не текстом: это контейнер, а не таблица.
///
/// Потолок тот же, что у импорта, и по той же причине: файл читается целиком.
pub fn read_backup_file(path: &Path) -> CoreResult<Vec<u8>> {
    let size = std::fs::metadata(path)
        .map_err(io_failed("Не удалось открыть выбранный файл."))?
        .len();
    if size > IMPORT_FILE_MAX_BYTES {
        return Err(CoreError::validation(
            "Файл слишком велик для резервной копии Syncra.",
        ));
    }
    std::fs::read(path).map_err(io_failed("Не удалось прочитать выбранный файл."))
}

/// Удалить файл. `false` — «не вышло»: на флешке и в чужой папке это
/// обычное дело, и валить из-за него команду нельзя.
pub fn remove_file(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => true,
        // Файла уже нет — обещание «удалим» выполнено, кем бы оно ни было.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Имена файлов и секций
// ---------------------------------------------------------------------------

/// `syncra-plain-2026-08-23.csv`. Префикс `plain` не для красоты: этот файл
/// потом ищут глазами в папке загрузок, чтобы удалить.
pub fn csv_file_name() -> String {
    format!("syncra-plain-{}.csv", today())
}

/// `syncra-2026-08-23.syncra`.
pub fn backup_file_name() -> String {
    format!("syncra-{}.syncra", today())
}

/// `Импорт 23.08` (`mock/transfer.ts:206`). Второй импорт в тот же день ложится
/// в ту же секцию.
pub fn import_vault_name() -> String {
    let now = Utc::now();
    format!("Импорт {:02}.{:02}", now.day(), now.month())
}

/// День в UTC — как и все даты ядра (`model::now_iso`). Расхождение с часовым
/// поясом человека на несколько часов в году безобидно: имя файла это подпись,
/// а не срок.
fn today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

// ---------------------------------------------------------------------------
// CSV-экспорт
// ---------------------------------------------------------------------------

/// Заголовок — диалект Chrome: его понимают все, кому этот файл предназначен.
///
/// Шестая колонка сверх диалекта намеренно: молча потерять при переезде ключи
/// подтверждения хуже, чем дать чужому импортёру лишний столбец, который он
/// пропустит.
const CSV_HEADER: [&str; 6] = ["name", "url", "username", "password", "note", "totp"];

/// Собрать CSV из всех ЖИВЫХ записей.
///
/// Включая локальные секции (`sync = 0`): это файл для переезда, а не
/// синхронизация, и «остаётся на устройстве» здесь ни при чём (`mock/index.ts:1429`).
pub fn build_csv(conn: &Connection, key: &VaultKey) -> CoreResult<(String, i64)> {
    let list = records::list(conn, key, None, false)?;

    let mut rows: Vec<Vec<String>> = Vec::with_capacity(list.len() + 1);
    rows.push(CSV_HEADER.iter().map(|name| (*name).to_owned()).collect());

    for meta in &list {
        let secrets = records::secrets(conn, key, &meta.record_id)?;
        rows.push(vec![
            meta.service_name.clone(),
            meta.urls.first().cloned().unwrap_or_default(),
            meta.login.clone(),
            secrets.password,
            note_column(meta, secrets.notes.as_deref()),
            secrets.totp_secret.unwrap_or_default(),
        ]);
    }

    Ok((csv::write(&rows), list.len() as i64))
}

/// В диалекте Chrome у записи один адрес, а в Syncra их до шестнадцати (§4.1).
/// Лишние дописываются в заметку: потерять их молча — значит отдать человеку
/// файл, по которому он не восстановит то, что у него было.
fn note_column(meta: &RecordMeta, notes: Option<&str>) -> String {
    let mut note = notes.unwrap_or_default().to_owned();
    if meta.urls.len() > 1 {
        if !note.is_empty() {
            note.push('\n');
        }
        note.push_str("Ещё адреса: ");
        note.push_str(&meta.urls[1..].join(", "));
    }
    note
}

// ---------------------------------------------------------------------------
// Бэкап
// ---------------------------------------------------------------------------

/// Снять с хранилища всё, что уезжает в бэкап.
pub fn collect_backup(conn: &Connection, key: &VaultKey) -> CoreResult<Backup> {
    let list = records::list(conn, key, None, false)?;
    let mut collected = Vec::with_capacity(list.len());

    for meta in &list {
        let secrets = records::secrets(conn, key, &meta.record_id)?;
        collected.push(BackupRecord {
            record_id: meta.record_id.clone(),
            vault_id: meta.vault_id.clone(),
            service_name: meta.service_name.clone(),
            urls: meta.urls.clone(),
            login: meta.login.clone(),
            account_label: meta.account_label.clone(),
            password: secrets.password,
            notes: secrets.notes,
            totp_secret: secrets.totp_secret,
            version: meta.version,
            created_at: meta.created_at.clone(),
            updated_at: meta.updated_at.clone(),
            password_updated_at: meta.password_updated_at.clone(),
        });
    }

    Ok(Backup {
        created_at: now_iso(),
        vaults: vaults::list(conn)?
            .into_iter()
            .map(|vault| BackupVault {
                vault_id: vault.vault_id,
                name: vault.name,
                color: vault.color,
                sync: vault.sync,
                is_default: vault.is_default,
                created_at: vault.created_at,
            })
            .collect(),
        records: collected,
        generator_profile: meta_json(conn, crate::storage::schema::META_GENERATOR_PROFILE)?,
        security_settings: meta_json(conn, crate::storage::schema::META_SECURITY_SETTINGS)?,
    })
}

fn meta_json(conn: &Connection, key: &str) -> CoreResult<Option<serde_json::Value>> {
    let raw: Option<Vec<u8>> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .ok();
    // Нечитаемая настройка не повод не сделать бэкап паролей: она сюда просто
    // не поедет, а хранилище восстановится на умолчаниях.
    Ok(raw.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

/// Разложить бэкап по таблицам уже созданного (в этой же транзакции) хранилища.
///
/// Секции и записи переезжают со СВОИМИ идентификаторами, версиями и датами:
/// восстановленное хранилище — то же самое хранилище, а не его копия с новой
/// историей. Иначе первая же встреча с уцелевшим устройством развела бы одну
/// папку на две, а каждую запись — на две спорящие (§5.2, §5.3).
pub fn restore_into(
    tx: &rusqlite::Transaction<'_>,
    key: &VaultKey,
    backup: &Backup,
) -> CoreResult<()> {
    // Секция по умолчанию обязана быть ровно одна — это инвариант уровня БД
    // (`vaults_one_default`). Если в файле её нет, ею становится первая: иначе
    // `create_record` без секции будет некуда положить запись.
    let has_default = backup.vaults.iter().any(|vault| vault.is_default);
    for (index, vault) in backup.vaults.iter().enumerate() {
        tx.execute(
            "INSERT INTO vaults (vault_id, name, color, sync, is_default, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                vault.vault_id,
                coerce_vault_name(&vault.name),
                crate::model::valid_vault_color(&vault.color)
                    .unwrap_or_else(|_| crate::model::VAULT_COLORS[0].to_owned()),
                vault.sync as i64,
                (vault.is_default || (!has_default && index == 0)) as i64,
                vault.created_at,
            ],
        )?;
    }

    for record in &backup.records {
        insert_restored(tx, key, record)?;
    }

    for (name, value) in [
        (
            crate::storage::schema::META_GENERATOR_PROFILE,
            backup.generator_profile.as_ref(),
        ),
        (
            crate::storage::schema::META_SECURITY_SETTINGS,
            backup.security_settings.as_ref(),
        ),
    ] {
        let Some(value) = value else { continue };
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![name, serde_json::to_vec(value)?],
        )?;
    }

    Ok(())
}

/// Запись из файла — прямой вставкой, а не через `records::create`: тот выдаёт
/// свой `record_id`, ставит `version = 1` и текущие даты, то есть заводит НОВУЮ
/// запись. Здесь же приезжает та же самая, и всё это должно уцелеть.
///
/// Секреты запечатываются ключом восстановленного хранилища под обычным AAD
/// полей (`crypto::field_aad`): `record_id` тот же, значит и место у шифротекста
/// то же.
fn insert_restored(
    tx: &rusqlite::Transaction<'_>,
    key: &VaultKey,
    record: &BackupRecord,
) -> CoreResult<()> {
    let id = &record.record_id;
    // Метаданные — тем же ключом и под тем же AAD, что у любой другой записи
    // (S7.1): `record_id` тот же, значит и место у шифротекста то же.
    let meta_ct = metadata::seal(
        key,
        metadata::Place::Record,
        id,
        &metadata::RecordFields {
            service_name: clip(&record.service_name, crate::model::META_FIELD_MAX_BYTES),
            urls: record.urls.clone(),
            login: clip(&record.login, crate::model::META_FIELD_MAX_BYTES),
            account_label: record.account_label.clone(),
        },
    )?;

    tx.execute(
        "INSERT INTO records (record_id, vault_id, meta_ct,
                              password_ct, notes_ct, totp_ct,
                              version, created_at, updated_at, password_updated_at, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
        rusqlite::params![
            id,
            record.vault_id,
            meta_ct,
            records::seal_field(key, id, SecretField::Password, Some(&record.password))?,
            records::seal_field(key, id, SecretField::Notes, record.notes.as_deref())?,
            records::seal_field(
                key,
                id,
                SecretField::TotpSecret,
                record.totp_secret.as_deref()
            )?,
            record.version,
            record.created_at,
            record.updated_at,
            record.password_updated_at,
        ],
    )?;
    Ok(())
}

/// Имя секции из файла не валидируется, а приводится — ровно как имя секции,
/// приехавшей от соседа (`vaults::adopt`): испорченное имя это повод нарисовать
/// папку иначе, а не отказать человеку в восстановлении.
fn coerce_vault_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "Секция из резервной копии".to_owned();
    }
    trimmed.chars().take(VAULT_NAME_MAX_LENGTH).collect()
}

// ---------------------------------------------------------------------------
// Импорт
// ---------------------------------------------------------------------------

/// Пары «адрес + логин», которые в хранилище уже есть. Снимаются ОДИН раз, до
/// прохода по файлу: дубликаты внутри самого файла между собой не схлопываются
/// (`mock/index.ts:1541`) — два аккаунта на один сервис это норма, а не
/// коллизия (§4.4).
pub fn known_pairs(conn: &Connection, key: &VaultKey) -> CoreResult<HashSet<String>> {
    let mut pairs = HashSet::new();
    for meta in records::list(conn, key, None, false)? {
        for url in &meta.urls {
            pairs.insert(import::pair_key(url, &meta.login));
        }
    }
    Ok(pairs)
}

/// Секция под импорт. Отдельная намеренно (§3.10 макета: «разберёте потом, не
/// смешивая с текущим»): триста чужих записей, высыпанных в «Личное», — это не
/// перенос, а беспорядок.
pub fn import_vault(conn: &Connection) -> CoreResult<crate::model::Vault> {
    let name = import_vault_name();
    match vaults::list(conn)?
        .into_iter()
        .find(|vault| vault.name == name)
    {
        Some(existing) => Ok(existing),
        None => vaults::create(conn, &name, IMPORT_VAULT_COLOR, false),
    }
}

/// Строка файла → черновик записи.
///
/// Метаданные ПРИВОДЯТСЯ, а не валидируются: чужой файл — это не форма, которую
/// человек заполняет и может исправить. Слишком длинное поле подрезается,
/// слишком длинный пароль превращает строку в «без пароля» — записи с обрезанным
/// паролем быть не может, она молча притворилась бы рабочей.
pub fn draft_from(entry: &ImportEntry, vault_id: &str) -> Option<crate::model::RecordDraft> {
    let host = import::import_host(&entry.site);
    let password = entry.password.to_string();
    if password.trim().is_empty() || password.len() > crate::model::SECRET_FIELD_MAX_BYTES {
        return None;
    }

    Some(crate::model::RecordDraft {
        vault_id: Some(vault_id.to_owned()),
        service_name: clip(
            &import::service_name_from_host(&host),
            crate::model::META_FIELD_MAX_BYTES,
        ),
        urls: if host.is_empty() {
            Vec::new()
        } else {
            vec![clip(&host, crate::model::META_FIELD_MAX_BYTES)]
        },
        login: clip(&entry.login, crate::model::META_FIELD_MAX_BYTES),
        account_label: None,
        password,
        notes: entry
            .notes
            .as_deref()
            .map(|text| clip(text, crate::model::SECRET_FIELD_MAX_BYTES)),
        totp_secret: entry
            .totp_secret
            .as_deref()
            .map(|text| clip(text, crate::model::SECRET_FIELD_MAX_BYTES)),
    })
}

/// Обрезка по БАЙТАМ, но по границе символа: потолки ядра считаются в байтах
/// (`model::within`), а разрезанный посередине символ — это уже не UTF-8.
fn clip(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// Имя сервиса у записи без адреса взять неоткуда — тогда им становится логин,
/// а совсем пустая строка записью не станет (`records::create` её отвергнет).
pub fn fallback_service_name(draft: &mut crate::model::RecordDraft) {
    if draft.service_name.trim().is_empty() {
        draft.service_name = draft.login.clone();
    }
}
