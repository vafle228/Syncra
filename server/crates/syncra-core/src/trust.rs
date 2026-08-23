//! Доверенные устройства и идентичность этого устройства (§2.1, §2.3, §8.3 «Trust»).
//!
//! Корень доверия — публичный ключ устройства, а не сетевой идентификатор:
//! MAC подделывается, IP меняется, а ключ — нет. Поэтому в `devices` лежит ключ,
//! и решение «пускать или нет» принимается по нему одному.
//!
//! Приватная половина пары живёт в `meta` **запечатанной ключом хранилища** —
//! то есть защищена мастер-паролем ровно так же, как пароли. Украденный файл
//! не даёт права представляться этим устройством.

use rusqlite::{Connection, OptionalExtension, Row, Transaction};
use sha2::{Digest, Sha256};

use crate::crypto::{self, DeviceKeypair, VaultKey};
use crate::error::{CoreError, CoreResult};
use crate::model::{now_iso, Device, DeviceKind, HostDevice, IsoDateTime};
use crate::storage::schema;

/// Сколько слов сверяет человек. Четыре — как в макете (§3.6) и в фейк-ядре
/// (`mock/pairing.ts:212`).
pub const FINGERPRINT_WORD_COUNT: usize = 4;

const COLUMNS: &str =
    "device_id, name, kind, fingerprint_words, paired_at, last_seen_at, revoked_at";

// ---------------------------------------------------------------------------
// Идентичность этого устройства
// ---------------------------------------------------------------------------

/// Есть ли у хранилища пара ключей и строка про себя в `devices`.
///
/// Обе половины проверяются вместе: хранилище с секретом, но без строки, — это
/// устройство, которого нет в собственном списке, и наоборот.
pub fn has_identity(conn: &Connection, device_id: &str) -> CoreResult<bool> {
    Ok(sealed_secret(conn)?.is_some() && find(conn, device_id, device_id)?.is_some())
}

/// Завести устройству пару ключей и строку в `devices`. Идемпотентна.
///
/// Зовётся из `init_vault` — в той же транзакции, что и остальная инициализация,
/// — и из `unlock`, если хранилище создавалось схемой 1, когда ключей не было
/// вовсе. Без второго пути такое хранилище навсегда осталось бы без права
/// представляться собой.
pub fn provision_identity(
    tx: &Transaction<'_>,
    vault_key: &VaultKey,
    host: &HostDevice,
    device_id: &str,
    at: &IsoDateTime,
) -> CoreResult<()> {
    if has_identity(tx, device_id)? {
        return Ok(());
    }

    let pair = DeviceKeypair::generate()?;
    let public_key = pair.public_key();

    tx.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![schema::META_DEVICE_SECRET, pair.seal(vault_key)?],
    )?;
    // `last_seen_at` при заведении равен `paired_at`: устройство на связи прямо
    // сейчас — им как раз пользуются.
    tx.execute(
        "INSERT INTO devices
           (device_id, name, kind, public_key, fingerprint_words,
            paired_at, last_seen_at, revoked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, NULL)",
        rusqlite::params![
            device_id,
            host.name,
            host.kind.as_str(),
            &public_key[..],
            serde_json::to_string(&fingerprint_words(&[&public_key]))?,
            at,
        ],
    )?;
    Ok(())
}

/// Пара ключей этого устройства. Понадобится сопряжению (S2) и рукопожатию (S3).
pub fn keypair(conn: &Connection, vault_key: &VaultKey) -> CoreResult<DeviceKeypair> {
    let blob = sealed_secret(conn)?.ok_or_else(|| CoreError::internal("Хранилище повреждено."))?;
    DeviceKeypair::open(vault_key, &blob)
}

fn sealed_secret(conn: &Connection) -> CoreResult<Option<Vec<u8>>> {
    Ok(conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [schema::META_DEVICE_SECRET],
            |row| row.get(0),
        )
        .optional()?)
}

/// Перешифровать секрет устройства новым ключом хранилища.
///
/// Зовётся из `change_master_password` в той же транзакции, что и перешифровка
/// записей: пропустить это — значит терять идентичность устройства при каждой
/// смене пароля, и заметить это удалось бы только в сети, через неделю.
pub fn rekey_secret(tx: &Transaction<'_>, from: &VaultKey, to: &VaultKey) -> CoreResult<()> {
    // Хранилище схемы 1 доезжает сюда без ключа — его заведёт ближайший `unlock`.
    let Some(blob) = sealed_secret(tx)? else {
        return Ok(());
    };

    tx.execute(
        "UPDATE meta SET value = ?2 WHERE key = ?1",
        rusqlite::params![
            schema::META_DEVICE_SECRET,
            crypto::reseal_device_secret(from, to, &blob)?
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Список и отзыв (F9)
// ---------------------------------------------------------------------------

fn from_row(row: &Row<'_>, this_device_id: &str) -> rusqlite::Result<Device> {
    let device_id: String = row.get("device_id")?;
    let words: String = row.get("fingerprint_words")?;
    let kind: String = row.get("kind")?;

    Ok(Device {
        is_this_device: device_id == this_device_id,
        device_id,
        name: row.get("name")?,
        kind: DeviceKind::parse_or_desktop(&kind),
        paired_at: row.get("paired_at")?,
        // Слова пришли из нашей же схемы; битый JSON здесь — испорченный файл, и
        // показать устройство без отпечатка лучше, чем уронить весь список.
        fingerprint_words: serde_json::from_str(&words).unwrap_or_default(),
        last_seen_at: row.get("last_seen_at")?,
        revoked_at: row.get("revoked_at")?,
    })
}

fn find(conn: &Connection, this_device_id: &str, device_id: &str) -> CoreResult<Option<Device>> {
    let sql = format!("SELECT {COLUMNS} FROM devices WHERE device_id = ?1");
    Ok(conn
        .query_row(&sql, [device_id], |row| from_row(row, this_device_id))
        .optional()?)
}

/// Все устройства, включая отозванные: отзыв — факт истории хранилища, а не
/// удаление строки. Человек, отозвавший украденный ноутбук, должен видеть, что
/// тот отозван, а не что его никогда не было.
///
/// Своё устройство идёт первым явным условием сортировки, а не по совпадению
/// времён: оно заводится при создании хранилища, но полагаться на это в
/// `ORDER BY` значило бы зависеть от того, чего схема не обещает.
pub fn list(conn: &Connection, this_device_id: &str) -> CoreResult<Vec<Device>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM devices
         ORDER BY device_id <> ?1, paired_at, device_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let devices = stmt
        .query_map([this_device_id], |row| from_row(row, this_device_id))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(devices)
}

/// Имя устройства для показа человеку.
///
/// Отдельно от [`list`] и [`find`], потому что зовётся оттуда, где `Device`
/// целиком не нужен: подпись под колонкой в экране конфликта (§5.5) — это одна
/// строка, а не карточка устройства.
pub fn name_of(conn: &Connection, device_id: &str) -> CoreResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT name FROM devices WHERE device_id = ?1",
            [device_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?)
}

/// Отозвать доступ (§2.3).
///
/// Идемпотентен, и `revoked_at` при повторе **не переписывается**: момент
/// отзыва — это факт, от которого считается, что успело разойтись по сети, а
/// что нет.
pub fn revoke(conn: &Connection, this_device_id: &str, device_id: &str) -> CoreResult<Device> {
    let device = find(conn, this_device_id, device_id)?
        .ok_or_else(|| CoreError::not_found("Устройство не найдено."))?;

    if device.is_this_device {
        // Устройство в руках — единственное, что у человека точно есть.
        return Err(CoreError::validation(
            "Нельзя отозвать устройство, на котором вы сейчас работаете.",
        ));
    }
    if device.revoked_at.is_some() {
        return Ok(device);
    }

    let revoked_at = now_iso();
    conn.execute(
        "UPDATE devices SET revoked_at = ?2 WHERE device_id = ?1",
        rusqlite::params![device_id, revoked_at],
    )?;
    Ok(Device {
        revoked_at: Some(revoked_at),
        ..device
    })
}

/// Второе устройство в том виде, в каком его записывает сопряжение (§2.2).
///
/// Публичный ключ приезжает в пейлоаде QR — по внеполосному каналу, а не по
/// сети: именно поэтому ему можно верить (§2.1).
pub struct PairedPeer<'a> {
    pub device_id: &'a str,
    pub name: &'a str,
    pub kind: DeviceKind,
    pub public_key: &'a [u8],
    /// Слова, которые человек только что сверил на двух экранах.
    pub fingerprint_words: &'a [String],
}

/// Записать сопряжённое устройство в доверенные (F8).
///
/// Повторное сопряжение того же устройства **снимает отзыв**: вернуть
/// отозванный ноутбук можно только новым сопряжением, и раз оно состоялось —
/// человек подтвердил его, сверив слова. Ключ при этом перезаписывается: у
/// заново сопряжённого устройства он может быть другим (хранилище пересоздали),
/// и держаться за старый значило бы не пускать того, кого только что впустили.
pub fn upsert_peer(
    conn: &Connection,
    peer: &PairedPeer<'_>,
    this_device_id: &str,
    at: &IsoDateTime,
) -> CoreResult<Device> {
    if peer.device_id == this_device_id {
        // Себя в доверенные вторым устройством не заводят: строка про себя
        // заводится вместе с хранилищем и переписывать её сопряжению нечем.
        return Err(CoreError::validation(
            "Это код этого же устройства. Нужен код второго — с его экрана.",
        ));
    }

    conn.execute(
        "INSERT INTO devices
           (device_id, name, kind, public_key, fingerprint_words,
            paired_at, last_seen_at, revoked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, NULL)
         ON CONFLICT(device_id) DO UPDATE SET
           name              = excluded.name,
           kind              = excluded.kind,
           public_key        = excluded.public_key,
           fingerprint_words = excluded.fingerprint_words,
           paired_at         = excluded.paired_at,
           last_seen_at      = excluded.last_seen_at,
           revoked_at        = NULL",
        rusqlite::params![
            peer.device_id,
            peer.name,
            peer.kind.as_str(),
            peer.public_key,
            serde_json::to_string(peer.fingerprint_words)?,
            at,
        ],
    )?;

    find(conn, this_device_id, peer.device_id)?
        .ok_or_else(|| CoreError::internal("Не удалось записать устройство."))
}

// ---------------------------------------------------------------------------
// Запросы сетевого слоя (S3)
// ---------------------------------------------------------------------------

/// Длина той половины публичного ключа, по которой устройство узнаётся в сети.
///
/// Корень доверия — ключ ПОДПИСИ (§2.1): им устройство доказывает, что оно
/// это оно. Вторая половина (X25519) участвует в обмене ключами, но не в
/// решении «пускать или нет», поэтому искать по ней нечего.
pub const SIGNING_PUBLIC_LEN: usize = 32;

/// Устройство, которому рукопожатие вправе поверить (§2.3).
///
/// Возвращается ТОЛЬКО для действующих: отозванное устройство отбивается здесь,
/// а не прячется в UI. Спека честно говорит, что отзыв не даёт стопроцентной
/// гарантии (§2.3), но то, что зависит от нас, обязано срабатывать.
pub fn authorized_peer(
    conn: &Connection,
    this_device_id: &str,
    signing_public: &[u8; SIGNING_PUBLIC_LEN],
) -> CoreResult<Option<Device>> {
    // Сравнение идёт в Rust, а не в SQL: устройств у человека единицы, индекс по
    // префиксу BLOB тут не окупается, зато `substr` по BLOB в SQLite ведёт себя
    // по-разному для текста и байтов — а ошибиться здесь значит пустить чужого.
    let sql = format!("SELECT {COLUMNS}, public_key FROM devices WHERE revoked_at IS NULL");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let stored: Vec<u8> = row.get("public_key")?;
        Ok((stored, from_row(row, this_device_id)?))
    })?;

    for row in rows {
        let (stored, device) = row?;
        // Своё же устройство пиром не считается: соединение с самим собой —
        // это не «сосед нашёлся», а петля мультикаста.
        if device.is_this_device {
            continue;
        }
        if stored.get(..SIGNING_PUBLIC_LEN) == Some(&signing_public[..]) {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

/// Публичный ключ конкретного устройства. Нужен сопряжению: пейлоад, приехавший
/// по сети, должен принадлежать тому же ключу, которым подписан канал.
pub fn peer_public_key(conn: &Connection, device_id: &str) -> CoreResult<Option<Vec<u8>>> {
    Ok(conn
        .query_row(
            "SELECT public_key FROM devices WHERE device_id = ?1",
            [device_id],
            |row| row.get(0),
        )
        .optional()?)
}

/// Отметить, что устройство только что было на связи (§5.1).
///
/// Двигает `last_seen_at` — то самое поле, которое до S3 стояло на дате
/// заведения и не менялось. Зовётся ТОЛЬКО после успешного рукопожатия: имя в
/// анонсе mDNS не доказывает ничего, а ключ доказывает.
///
/// Отозванному устройству время не двигаем: «последний раз на связи» у него —
/// это момент, когда оно ещё было своим.
pub fn touch_last_seen(
    conn: &Connection,
    this_device_id: &str,
    device_id: &str,
    at: &IsoDateTime,
) -> CoreResult<Option<Device>> {
    let updated = conn.execute(
        "UPDATE devices SET last_seen_at = ?2 WHERE device_id = ?1 AND revoked_at IS NULL",
        rusqlite::params![device_id, at],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    find(conn, this_device_id, device_id)
}

/// Сколько устройств придётся обновить после смены мастер-пароля: действующие,
/// кроме своего. Отозванным обновление не поедет — им вообще больше ничего не
/// поедет (§2.3).
pub fn active_peer_count(conn: &Connection, this_device_id: &str) -> CoreResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM devices WHERE revoked_at IS NULL AND device_id <> ?1",
        [this_device_id],
        |row| row.get(0),
    )?)
}

// ---------------------------------------------------------------------------
// Отпечаток (§2.2)
// ---------------------------------------------------------------------------

/// Слова отпечатка — детерминированно из переданных ключей.
///
/// С одним ключом это «как называется вот это устройство»; с двумя, через
/// [`pair_fingerprint`], — отпечаток сеанса сопряжения. Совпадение слов на двух
/// экранах и есть защита от MITM: у человека посередине ключи другие, а значит
/// и слова другие.
///
/// Куски хешируются с длиной впереди, чтобы `("ab", "c")` и `("a", "bc")` не
/// давали один отпечаток. Словарь — тот же, что у генератора: заводить второй
/// список русских слов незачем.
pub fn fingerprint_words(parts: &[&[u8]]) -> Vec<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"syncra:fingerprint:v1");
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    let digest = hasher.finalize();

    let words = crate::generator::words();
    digest
        .chunks_exact(2)
        .take(FINGERPRINT_WORD_COUNT)
        .map(|pair| {
            let index = u16::from_be_bytes([pair[0], pair[1]]) as usize;
            words[index % words.len()].to_owned()
        })
        .collect()
}

/// Отпечаток СЕАНСА сопряжения: слова из публичных ключей обеих сторон (§2.2).
///
/// Ключи сортируются, потому что порядок сторон не определён: одно устройство
/// читает код другого, и кто из них «первый» — вопрос без ответа. Без сортировки
/// два экрана показали бы разные слова, и сверять было бы нечего.
pub fn pair_fingerprint(local_public: &[u8], remote_public: &[u8]) -> Vec<String> {
    let (first, second) = if local_public <= remote_public {
        (local_public, remote_public)
    } else {
        (remote_public, local_public)
    };
    fingerprint_words(&[first, second])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_and_the_right_length() {
        let words = fingerprint_words(&["ключ".as_bytes()]);

        assert_eq!(words.len(), FINGERPRINT_WORD_COUNT);
        assert_eq!(words, fingerprint_words(&["ключ".as_bytes()]));
    }

    #[test]
    fn fingerprint_changes_with_any_of_the_keys() {
        let both = fingerprint_words(&[b"first", b"second"]);

        // Подмена любой из сторон меняет слова — иначе сверять было бы нечего.
        assert_ne!(both, fingerprint_words(&[b"other", b"second"]));
        assert_ne!(both, fingerprint_words(&[b"first", b"other"]));
        // ...и склейка кусков не выдаёт себя за них же.
        assert_ne!(both, fingerprint_words(&[b"firstsecond"]));
    }
}
