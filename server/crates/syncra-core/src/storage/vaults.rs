//! Секции (§4.2, F7).
//!
//! Секция — папка, а не отдельное хранилище: ключ шифрования у всех секций один,
//! и единственное, что секция правда решает, — уезжает ли её содержимое на другие
//! устройства (`sync`).

use rusqlite::{Connection, OptionalExtension, Row};

use crate::error::{CoreError, CoreResult};
use crate::model::{now_iso, valid_vault_color, valid_vault_name, Vault, VaultPatch};

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
