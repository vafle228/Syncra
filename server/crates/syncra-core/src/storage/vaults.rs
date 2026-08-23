//! Секции (§4.2, F7).
//!
//! Секция — папка, а не отдельное хранилище: ключ шифрования у всех секций один,
//! и единственное, что секция правда решает, — уезжает ли её содержимое на другие
//! устройства (`sync`).

use rusqlite::{Connection, OptionalExtension, Row};

use crate::error::{CoreError, CoreResult};
use crate::model::{
    now_iso, valid_vault_color, valid_vault_name, Vault, VaultPatch, VAULT_COLORS,
    VAULT_NAME_MAX_LENGTH,
};

fn from_row(row: &Row<'_>) -> rusqlite::Result<Vault> {
    Ok(Vault {
        vault_id: row.get("vault_id")?,
        name: row.get("name")?,
        color: row.get("color")?,
        sync: row.get::<_, i64>("sync")? != 0,
        is_default: row.get::<_, i64>("is_default")? != 0,
        created_at: row.get("created_at")?,
    })
}

/// Порядок списка — порядок создания: сайдбар не должен перетасовываться при
/// каждом переименовании. `rowid` в довесок к дате разводит секции, заведённые
/// в одну миллисекунду.
const SELECT_ALL: &str = "SELECT vault_id, name, color, sync, is_default, created_at
                          FROM vaults ORDER BY created_at, rowid";

pub fn list(conn: &Connection) -> CoreResult<Vec<Vault>> {
    let mut stmt = conn.prepare(SELECT_ALL)?;
    let vaults = stmt
        .query_map([], from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(vaults)
}

pub fn find(conn: &Connection, vault_id: &str) -> CoreResult<Vault> {
    conn.query_row(
        "SELECT vault_id, name, color, sync, is_default, created_at
         FROM vaults WHERE vault_id = ?1",
        [vault_id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| CoreError::not_found("Секция не найдена."))
}

pub fn default_vault(conn: &Connection) -> CoreResult<Vault> {
    conn.query_row(
        "SELECT vault_id, name, color, sync, is_default, created_at
         FROM vaults WHERE is_default = 1",
        [],
        from_row,
    )
    .optional()?
    .ok_or_else(|| CoreError::internal("В хранилище нет секции по умолчанию."))
}

/// Завести секцию. `is_default` не выставляется никогда: секция по умолчанию
/// назначается отдельной командой, и молча переносить точку приземления новых
/// записей при создании соседней папки было бы сюрпризом.
pub fn create(conn: &Connection, name: &str, color: &str, is_default: bool) -> CoreResult<Vault> {
    let vault = Vault {
        vault_id: uuid::Uuid::new_v4().to_string(),
        name: valid_vault_name(name)?,
        color: valid_vault_color(color)?,
        // Новая секция синхронизируется: продукт про синхронизацию, и молча
        // оставлять записи на одном устройстве было бы сюрпризом. Выключить —
        // тумблером, осознанно.
        sync: true,
        is_default,
        created_at: now_iso(),
    };

    conn.execute(
        "INSERT INTO vaults (vault_id, name, color, sync, is_default, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            vault.vault_id,
            vault.name,
            vault.color,
            vault.sync as i64,
            vault.is_default as i64,
            vault.created_at,
        ],
    )?;

    Ok(vault)
}

/// Завести секцию с ЧУЖИМ `vault_id` — так приезжает секция по синхронизации.
///
/// Отдельно от [`create`], потому что `create` выдаёт свой UUID, а здесь
/// идентификатор обязан совпасть с тем, что у соседа: иначе одна и та же папка
/// разъедется на две и записи начнут ходить кругами.
///
/// Имя и цвет приходят от соседа, а не от человека, и **не валидируются, а
/// приводятся**: незнакомый цвет и слишком длинное имя — повод нарисовать
/// секцию иначе, а не оборвать обмен на середине.
///
/// `sync = 1`: секция приехала по синхронизации, значит синхронизируется. Выключить
/// её тумблером человек может и потом — и тогда она перестанет и уезжать, и приезжать.
/// `is_default = 0`: точку приземления новых записей сосед не выбирает.
pub fn adopt(conn: &Connection, vault_id: &str, name: &str, color: &str) -> CoreResult<Vault> {
    if let Some(existing) = find_optional(conn, vault_id)? {
        return Ok(existing);
    }

    let vault = Vault {
        vault_id: vault_id.to_owned(),
        name: coerce_name(name),
        color: coerce_color(color),
        sync: true,
        is_default: false,
        created_at: now_iso(),
    };

    conn.execute(
        "INSERT INTO vaults (vault_id, name, color, sync, is_default, created_at)
         VALUES (?1, ?2, ?3, 1, 0, ?4)",
        rusqlite::params![vault.vault_id, vault.name, vault.color, vault.created_at],
    )?;

    Ok(vault)
}

/// Секция или `None` — без ошибки: вызывающему интересно именно «а есть ли».
pub fn find_optional(conn: &Connection, vault_id: &str) -> CoreResult<Option<Vault>> {
    let sql = "SELECT vault_id, name, color, sync, is_default, created_at
               FROM vaults WHERE vault_id = ?1";
    Ok(conn.query_row(sql, [vault_id], from_row).optional()?)
}

/// Имя соседа как есть не берём: пустое подменяем, длинное подрезаем — ровно как
/// [`crate::model::HostDevice::new`] поступает с именем машины из ОС.
fn coerce_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return REMOTE_VAULT_NAME.to_owned();
    }
    trimmed.chars().take(VAULT_NAME_MAX_LENGTH).collect()
}

/// Цвет — ступень нашей палитры. Незнакомая ступень значит, что сосед новее нас;
/// показать секцию первым цветом честнее, чем не показать её вовсе.
fn coerce_color(color: &str) -> String {
    if VAULT_COLORS.contains(&color) {
        color.to_owned()
    } else {
        VAULT_COLORS[0].to_owned()
    }
}

/// Чем подписывается приехавшая секция, у которой не оказалось имени.
const REMOTE_VAULT_NAME: &str = "Секция с другого устройства";

/// Переименовать / перекрасить. Флаг синхронизации сюда намеренно не входит.
pub fn update(conn: &Connection, vault_id: &str, patch: &VaultPatch) -> CoreResult<Vault> {
    let current = find(conn, vault_id)?;

    let name = match patch.name.as_deref() {
        Some(value) => valid_vault_name(value)?,
        None => current.name,
    };
    let color = match patch.color.as_deref() {
        Some(value) => valid_vault_color(value)?,
        None => current.color,
    };

    conn.execute(
        "UPDATE vaults SET name = ?2, color = ?3 WHERE vault_id = ?1",
        rusqlite::params![vault_id, name, color],
    )?;

    find(conn, vault_id)
}

pub fn set_sync(conn: &Connection, vault_id: &str, sync: bool) -> CoreResult<Vault> {
    find(conn, vault_id)?;
    conn.execute(
        "UPDATE vaults SET sync = ?2 WHERE vault_id = ?1",
        rusqlite::params![vault_id, sync as i64],
    )?;
    find(conn, vault_id)
}

/// Назначить секцию по умолчанию. Возвращает ВЕСЬ список: флаг сняли ещё с одной,
/// и без соседей UI пришлось бы догадываться, с кого именно.
pub fn set_default(conn: &mut Connection, vault_id: &str) -> CoreResult<Vec<Vault>> {
    let tx = conn.transaction()?;
    find(&tx, vault_id)?;

    // Сначала снимаем со всех, потом ставим одному: частичный уникальный индекс
    // проверяется после каждого оператора, и обратный порядок упёрся бы в него.
    tx.execute("UPDATE vaults SET is_default = 0 WHERE is_default = 1", [])?;
    tx.execute(
        "UPDATE vaults SET is_default = 1 WHERE vault_id = ?1",
        [vault_id],
    )?;

    let vaults = list(&tx)?;
    tx.commit()?;
    Ok(vaults)
}

/// Удалить секцию (F7).
///
/// Записи НЕ удаляются вместе с ней (§4.2 — секция это папка): они переезжают в
/// секцию по умолчанию. Переезд — обычное изменение записи, поэтому `version`
/// растёт: на другом устройстве это должно быть видно как правка, а не как
/// молчаливая подмена.
pub fn delete(conn: &mut Connection, vault_id: &str) -> CoreResult<Vec<Vault>> {
    let tx = conn.transaction()?;

    let doomed = find(&tx, vault_id)?;
    if doomed.is_default {
        return Err(CoreError::validation(
            "Секция по умолчанию не удаляется: новым записям нужно куда-то попадать.",
        ));
    }

    let fallback = default_vault(&tx)?;
    tx.execute(
        "UPDATE records
         SET vault_id = ?2, version = version + 1, updated_at = ?3
         WHERE vault_id = ?1",
        rusqlite::params![vault_id, fallback.vault_id, now_iso()],
    )?;
    tx.execute("DELETE FROM vaults WHERE vault_id = ?1", [vault_id])?;

    let vaults = list(&tx)?;
    tx.commit()?;
    Ok(vaults)
}
