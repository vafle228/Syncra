//! Дифференциальная синхронизация: манифест, дифф, надгробия (F10, §5.3 · §5.4).
//!
//! Здесь живут форма манифеста, правило свежести и весь SQL вокруг обмена. Ни
//! сокетов, ни кадров: как это едет по проводу — дело [`crate::net::sync`], а
//! что именно едет — дело этого модуля. Прецедент раскладки — [`crate::trust`]:
//! модуль верхнего уровня, который сам ходит в `Connection`.
//!
//! # Три правила, из которых всё остальное следует
//!
//! **Свежесть решает `version`, а не время** (§5.2). Часы на двух устройствах
//! расходятся, счётчик — нет. `updated_at` едет рядом, но только чтобы показать
//! его человеку.
//!
//! **Шифротексты не копируются.** Мастер-пароли на двух устройствах разные,
//! значит разные и ключи хранилищ. Отправитель распечатывает секрет своим
//! ключом, канал шифрует его на сессионный (§3.2), получатель запечатывает
//! **своим** с правильным AAD. Открытый текст живёт только внутри процесса и
//! границу IPC не пересекает — Закон №1 цел, четвёртой команды не появилось.
//!
//! **Надгробие — такая же запись** (§5.4). Не поехавшее удаление воскресает с
//! другого устройства, а это худшая из возможных поломок менеджера паролей.
//!
//! **Свежесть — не то же самое, что победа.** Когда обе стороны ушли от общего
//! предка, «больше версия — та и права» перестаёт быть ответом: чья-то правка
//! при этом пропадёт молча. Такое расхождение сворачивается в конфликт и ждёт
//! человека — [`conflicts`], §5.5.

pub mod conflicts;

use std::collections::{HashMap, HashSet};
use std::fmt;

use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::crypto::VaultKey;
use crate::error::CoreResult;
use crate::model::{now_iso, IsoDateTime, RecordId, SecretField, VaultId};
use crate::storage::{records, vaults};

// ---------------------------------------------------------------------------
// Пороги
// ---------------------------------------------------------------------------

/// Сколько записей манифеста едет одним кадром.
///
/// Кадр ограничен мегабайтом (`net::wire::MAX_FRAME`), а запись манифеста — это
/// два UUID и три коротких строки, около полутора сотен байт. Двести пятьдесят
/// шесть штук укладываются с двадцатикратным запасом.
pub const MANIFEST_BATCH: usize = 256;

/// Сколько записей диффа едет одним кадром — по счёту.
pub const RECORD_BATCH: usize = 16;

/// ...и по размеру. Запись с длинной заметкой весит непредсказуемо, поэтому
/// порция закрывается по тому порогу, который наступит раньше.
pub const RECORD_BATCH_BYTES: usize = 512 * 1024;

/// Сколько записей можно попросить за один круг.
///
/// Первая синхронизация с новым устройством просит всё хранилище сразу, и
/// список идентификаторов сам по себе кадр переполнить может. Остаток приедет
/// следующим кругом — обмен от этого не ломается, просто идёт в два захода.
pub const WANT_LIMIT: usize = 4096;

// ---------------------------------------------------------------------------
// Манифест
// ---------------------------------------------------------------------------

/// Строка манифеста: чем устройство отчитывается по одной записи (§5.3).
///
/// Секретов здесь нет и быть не может — это разговор о версиях, а не о паролях.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub record_id: RecordId,
    /// Секция записи. Едет затем, чтобы собеседник мог применить **своё**
    /// правило §4.2: если эта секция у него локальная, запись он не попросит.
    pub vault_id: VaultId,
    pub version: i64,
    /// Для показа человеку и для S5. В решении о свежести не участвует (§5.2).
    pub updated_at: IsoDateTime,
    /// Надгробие (§5.4). Едет наравне с живой записью.
    pub deleted_at: Option<IsoDateTime>,
    /// Уезжает ли запись с этого устройства (`vaults.sync`, §4.2).
    ///
    /// Строки локальных секций в манифесте **есть** — и это не противоречие.
    /// Без них собеседник видел бы только «у него этой записи нет» и слал бы её
    /// заново каждую минуту до конца времён. `false` означает ровно «она у меня
    /// есть, но никуда не поедет: не шли и не жди».
    pub shared: bool,
}

/// Манифест целиком.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
    /// Секции, выключенные из синхронизации на этой стороне.
    ///
    /// Отдельным списком, потому что по записям это не восстанавливается:
    /// у пустой локальной секции строк в манифесте нет вовсе, а слать в неё
    /// записи всё равно нельзя.
    pub local_vaults: Vec<VaultId>,
}

/// Что делать по итогам сравнения манифестов.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncPlan {
    /// Записи, которых у собеседника нет или которые у него устарели.
    pub send: Vec<RecordId>,
    /// Записи, которые устарели у нас.
    pub want: Vec<RecordId>,
    /// Записи, по которым обе стороны уже сошлись: ни посылать, ни просить
    /// нечего. Их версия и есть общий предок — то, что S5 будет сравнивать,
    /// решая, конфликт перед ним или обычное обновление (§5.5).
    pub settled: Vec<(RecordId, i64)>,
    /// Влез ли [`SyncPlan::want`] целиком. Обрезанный круг **не** закрывает
    /// отметки: часть записей просто не успела приехать, и объявлять их
    /// сошедшимися нельзя.
    pub want_complete: bool,
}

// ---------------------------------------------------------------------------
// Запись на проводе
// ---------------------------------------------------------------------------

/// Запись целиком, как она едет между устройствами.
///
/// Секреты здесь **открытым текстом** — и это не дыра, а единственный
/// возможный порядок: шифротексты сторон несовместимы (см. шапку модуля).
/// Живёт эта структура ровно от распечатки у отправителя до запечатывания у
/// получателя, внутри зашифрованного канала.
///
/// Секция едет рядом с записью: `vault_id` у обеих сторон один и тот же, а имя
/// и цвет нужны, чтобы приехавшая запись легла в узнаваемую папку, а не в чужую.
#[derive(Clone, PartialEq, Eq)]
pub struct SyncRecord {
    pub record_id: RecordId,
    pub vault_id: VaultId,
    pub vault_name: String,
    pub vault_color: String,
    pub service_name: String,
    pub urls: Vec<String>,
    pub login: String,
    pub account_label: Option<String>,
    /// `None` у надгробия: секреты стёрты при удалении, а не ждут чистки.
    pub password: Option<String>,
    pub notes: Option<String>,
    pub totp_secret: Option<String>,
    pub version: i64,
    pub created_at: IsoDateTime,
    pub updated_at: IsoDateTime,
    pub password_updated_at: IsoDateTime,
    pub deleted_at: Option<IsoDateTime>,
}

/// `Debug` написан руками намеренно.
///
/// `#[derive(Debug)]` на структуре с паролем — это заготовка утечки: одна
/// строчка отладочной печати в чужом коде, и пароль лежит в логе. Здесь
/// печатается наличие секрета, а не секрет.
impl fmt::Debug for SyncRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SyncRecord")
            .field("record_id", &self.record_id)
            .field("vault_id", &self.vault_id)
            .field("version", &self.version)
            .field("deleted_at", &self.deleted_at)
            .field("password", &secret_shape(&self.password))
            .field("notes", &secret_shape(&self.notes))
            .field("totp_secret", &secret_shape(&self.totp_secret))
            .finish_non_exhaustive()
    }
}

fn secret_shape(value: &Option<String>) -> &'static str {
    match value {
        Some(_) => "<скрыт>",
        None => "<нет>",
    }
}

// ---------------------------------------------------------------------------
// Правило свежести (§5.2)
// ---------------------------------------------------------------------------

/// Что делать с приехавшей версией.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Приехавшая свежее — кладём её.
    Take,
    /// Местная не старее — не трогаем ничего.
    Keep,
    /// Обе стороны ушли от общего предка. Молча тут не решить ни в чью пользу:
    /// любой выбор потеряет чью-то правку (§5.5).
    Conflict,
}

/// Побеждает ли приехавшая версия местную — правило свежести без предка.
///
/// Живёт отдельно от [`classify`], потому что это ровно то, что остаётся, когда
/// общей точки отсчёта нет: сравнение двух счётчиков и больше ничего (§5.2).
pub fn remote_wins(remote_version: i64, local_version: Option<i64>) -> bool {
    match local_version {
        // Записи нет вовсе — берём. Надгробие тоже: удаление должно доехать до
        // устройства, которое про запись ещё не слышало (§5.4).
        None => true,
        Some(local) => remote_version > local,
    }
}

/// Единственное место, где ядро решает судьбу приехавшей версии.
///
/// Вердикт считается по ТРЁМ числам, а не по двум: третье — общий предок
/// (`sync_state.synced_version`), версия, на которой стороны сошлись в прошлый
/// раз. Без него «обе правили» неотличимо от «один правил», и расхождение
/// разрешалось бы тихой перезаписью.
///
/// **Предка нет — конфликта нет.** Отметки стираются при отзыве и при
/// (повторном) сопряжении (см. [`forget_device`]), а вернувшийся ноутбук
/// получает хранилище заново. Считать расхождением каждую его запись значило бы
/// встречать человека сотней конфликтов там, где на деле просто новое
/// знакомство.
///
/// Содержимое здесь не сравнивается намеренно: это чистая функция от трёх
/// чисел. Совпали ли записи на самом деле, выясняет [`apply`] — там есть ключ
/// хранилища, а здесь его быть не должно.
pub fn classify(remote_version: i64, local_version: Option<i64>, ancestor: Option<i64>) -> Verdict {
    let (Some(local), Some(ancestor)) = (local_version, ancestor) else {
        return take_or_keep(remote_wins(remote_version, local_version));
    };

    if local > ancestor && remote_version > ancestor {
        Verdict::Conflict
    } else {
        take_or_keep(remote_version > local)
    }
}

fn take_or_keep(wins: bool) -> Verdict {
    if wins {
        Verdict::Take
    } else {
        Verdict::Keep
    }
}

/// Версия, на которой мы в прошлый раз сошлись с этим устройством по этой
/// записи. `None` — не сходились ни разу.
pub fn ancestor(conn: &Connection, device_id: &str, record_id: &str) -> CoreResult<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT synced_version FROM sync_state WHERE device_id = ?1 AND record_id = ?2",
            rusqlite::params![device_id, record_id],
            |row| row.get(0),
        )
        .optional()?)
}

// ---------------------------------------------------------------------------
// Манифест: чтение и сравнение
// ---------------------------------------------------------------------------

/// Собрать манифест этого хранилища.
pub fn manifest(conn: &Connection) -> CoreResult<Manifest> {
    let mut stmt = conn.prepare(
        "SELECT r.record_id, r.vault_id, r.version, r.updated_at, r.deleted_at, v.sync
         FROM records r JOIN vaults v ON v.vault_id = r.vault_id
         ORDER BY r.record_id",
    )?;
    let entries = stmt
        .query_map([], |row| {
            Ok(ManifestEntry {
                record_id: row.get("record_id")?,
                vault_id: row.get("vault_id")?,
                version: row.get("version")?,
                updated_at: row.get("updated_at")?,
                deleted_at: row.get("deleted_at")?,
                shared: row.get::<_, i64>("sync")? != 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    Ok(Manifest {
        entries,
        local_vaults: local_vaults(conn)?,
    })
}

/// Секции, выключенные из синхронизации.
pub fn local_vaults(conn: &Connection) -> CoreResult<Vec<VaultId>> {
    let mut stmt = conn.prepare("SELECT vault_id FROM vaults WHERE sync = 0 ORDER BY vault_id")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

/// Сравнить манифест собеседника со своим состоянием (§5.3, шаг 2).
///
/// Обе стороны считают это по одному и тому же правилу, поэтому «что я пошлю»
/// у одной совпадает с «что я попрошу» у другой — договариваться об этом
/// отдельно не нужно.
///
/// `device_id` нужен ради общего предка: без него расхождение неотличимо от
/// обычного обновления, и `want` не попросил бы того, из чего конфликт можно
/// поднять (§5.5).
pub fn plan(conn: &Connection, device_id: &str, remote: &Manifest) -> CoreResult<SyncPlan> {
    let local = manifest(conn)?;

    let remote_by_id: HashMap<&str, &ManifestEntry> = index_by_id(&remote.entries);
    let local_by_id: HashMap<&str, &ManifestEntry> = index_by_id(&local.entries);
    let remote_local_vaults: HashSet<&str> = id_set(&remote.local_vaults);
    let our_local_vaults: HashSet<&str> = id_set(&local.local_vaults);

    let mut send = Vec::new();
    for entry in local.entries.iter().filter(|entry| entry.shared) {
        match remote_by_id.get(entry.record_id.as_str()) {
            // Есть, но у него локальная: §4.2 действует и в эту сторону.
            Some(peer) if !peer.shared => {}
            Some(peer) if peer.version < entry.version => send.push(entry.record_id.clone()),
            Some(_) => {}
            // Записи у него нет. Секция при этом может быть — и быть локальной;
            // тогда посылка не приживётся, и слать её незачем.
            None if remote_local_vaults.contains(entry.vault_id.as_str()) => {}
            None => send.push(entry.record_id.clone()),
        }
    }

    let open_conflicts = conflicts::open_ids(conn)?;

    let mut want = Vec::new();
    for entry in remote.entries.iter().filter(|entry| entry.shared) {
        // Своё правило §4.2 каждая сторона применяет сама: незачем просить то,
        // что здесь всё равно не приживётся.
        if our_local_vaults.contains(entry.vault_id.as_str()) {
            continue;
        }
        let Some(mine) = local_by_id.get(entry.record_id.as_str()) else {
            want.push(entry.record_id.clone());
            continue;
        };
        if !mine.shared {
            continue;
        }

        let ancestor = ancestor(conn, device_id, &entry.record_id)?;
        match classify(entry.version, Some(mine.version), ancestor) {
            Verdict::Take => want.push(entry.record_id.clone()),
            Verdict::Keep => {}
            // Расхождение просим ОТДЕЛЬНО от свежести: при равных счётчиках
            // чужая версия иначе не приедет вовсе, и поднимать конфликт будет
            // не из чего. Кроме случая, когда тот же самый спор уже висит: он
            // разобран, и возить его по проводу каждую минуту незачем.
            Verdict::Conflict if standing(conn, &entry.record_id, mine.version, entry.version)? => {
            }
            Verdict::Conflict => want.push(entry.record_id.clone()),
        }
    }

    let want_complete = want.len() <= WANT_LIMIT;
    want.truncate(WANT_LIMIT);

    let settled = if want_complete {
        settled_from(&local.entries, &send, &want, &open_conflicts)
    } else {
        Vec::new()
    };

    Ok(SyncPlan {
        send,
        want,
        settled,
        want_complete,
    })
}

/// Тот же спор, что уже лежит: обе стороны с тех пор не менялись.
fn standing(
    conn: &Connection,
    record_id: &str,
    local_version: i64,
    remote_version: i64,
) -> CoreResult<bool> {
    Ok(conflicts::open_for(conn, record_id)?
        .is_some_and(|open| open.local_version == local_version && open.version == remote_version))
}

/// «Ни туда, ни оттуда» — значит, сошлись.
///
/// Считается вычитанием, а не отдельным проходом по манифесту собеседника:
/// каждая наша синкаемая запись либо едет к нему, либо ждёт его версию, либо
/// уже совпадает. Третий случай и есть отметка.
///
/// Спорные записи из отметок **вычитаются**, и это не мелочь: отметка — это
/// общий предок, а объявить предком текущую версию значило бы стереть само
/// расхождение. Одна такая отметка — и конфликт становится неотличим от
/// сходимости навсегда.
fn settled_from(
    local: &[ManifestEntry],
    send: &[RecordId],
    want: &[RecordId],
    disputed: &HashSet<RecordId>,
) -> Vec<(RecordId, i64)> {
    let moving: HashSet<&str> = send.iter().chain(want.iter()).map(String::as_str).collect();

    local
        .iter()
        .filter(|entry| {
            entry.shared
                && !moving.contains(entry.record_id.as_str())
                && !disputed.contains(&entry.record_id)
        })
        .map(|entry| (entry.record_id.clone(), entry.version))
        .collect()
}

/// То же самое, но для стороны, которая чужого манифеста не видела.
///
/// Инициатор круга знает только две вещи: что у него попросили и что ему
/// прислали. Остальное собеседник не тронул — значит, у него оно не старее.
pub fn settled_after(
    conn: &Connection,
    wanted: &[RecordId],
    received: &[RecordId],
) -> CoreResult<Vec<(RecordId, i64)>> {
    let local = manifest(conn)?;
    let disputed = conflicts::open_ids(conn)?;
    Ok(settled_from(&local.entries, wanted, received, &disputed))
}

fn index_by_id(entries: &[ManifestEntry]) -> HashMap<&str, &ManifestEntry> {
    entries
        .iter()
        .map(|entry| (entry.record_id.as_str(), entry))
        .collect()
}

fn id_set(ids: &[VaultId]) -> HashSet<&str> {
    ids.iter().map(String::as_str).collect()
}

// ---------------------------------------------------------------------------
// Выгрузка и применение
// ---------------------------------------------------------------------------

/// Собрать записи для отправки, распечатав секреты своим ключом.
pub fn export(conn: &Connection, key: &VaultKey, ids: &[RecordId]) -> CoreResult<Vec<SyncRecord>> {
    let mut stmt = conn.prepare(
        "SELECT r.vault_id, v.name AS vault_name, v.color AS vault_color, v.sync,
                r.service_name, r.urls, r.login, r.account_label,
                r.password_ct, r.notes_ct, r.totp_ct,
                r.version, r.created_at, r.updated_at, r.password_updated_at, r.deleted_at
         FROM records r JOIN vaults v ON v.vault_id = r.vault_id
         WHERE r.record_id = ?1",
    )?;

    let mut out = Vec::with_capacity(ids.len());
    for record_id in ids {
        // Запись могла исчезнуть или её секция — стать локальной, пока круг шёл.
        // Тогда её просто не будет в порции: §4.2 — обещание, а не оптимизация.
        let Ok(row) = stmt.query_row([record_id], StoredForSync::from_row) else {
            continue;
        };
        if !row.shared {
            continue;
        }
        out.push(row.into_record(key, record_id)?);
    }

    Ok(out)
}

/// Строка выгрузки до расшифровки. Отдельный тип, чтобы шестнадцать полей не
/// разъезжались по индексам кортежа.
struct StoredForSync {
    vault_id: VaultId,
    vault_name: String,
    vault_color: String,
    shared: bool,
    service_name: String,
    urls: String,
    login: String,
    account_label: Option<String>,
    password_ct: Option<Vec<u8>>,
    notes_ct: Option<Vec<u8>>,
    totp_ct: Option<Vec<u8>>,
    version: i64,
    created_at: IsoDateTime,
    updated_at: IsoDateTime,
    password_updated_at: IsoDateTime,
    deleted_at: Option<IsoDateTime>,
}

impl StoredForSync {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            vault_id: row.get("vault_id")?,
            vault_name: row.get("vault_name")?,
            vault_color: row.get("vault_color")?,
            shared: row.get::<_, i64>("sync")? != 0,
            service_name: row.get("service_name")?,
            urls: row.get("urls")?,
            login: row.get("login")?,
            account_label: row.get("account_label")?,
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

    /// Распечатать секреты СВОИМ ключом. Дальше их зашифрует канал, а на той
    /// стороне запечатает уже чужой ключ хранилища — шифротексты не совместимы.
    fn into_record(self, key: &VaultKey, record_id: &RecordId) -> CoreResult<SyncRecord> {
        Ok(SyncRecord {
            record_id: record_id.clone(),
            vault_id: self.vault_id,
            vault_name: self.vault_name,
            vault_color: self.vault_color,
            service_name: self.service_name,
            urls: serde_json::from_str(&self.urls).unwrap_or_default(),
            login: self.login,
            account_label: self.account_label,
            password: records::open_field(
                key,
                record_id,
                SecretField::Password,
                &self.password_ct,
            )?,
            notes: records::open_field(key, record_id, SecretField::Notes, &self.notes_ct)?,
            totp_secret: records::open_field(
                key,
                record_id,
                SecretField::TotpSecret,
                &self.totp_ct,
            )?,
            version: self.version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            password_updated_at: self.password_updated_at,
            deleted_at: self.deleted_at,
        })
    }
}

/// Что легло по итогам диффа.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// Записи, которые правда применились, с их новыми версиями. Именно они —
    /// и только они — двигают общий предок.
    pub records: Vec<(RecordId, i64)>,
    /// Записи, по которым поднялось НОВОЕ расхождение. Про них уходит
    /// `conflict_raised`; повторно приехавший тот же спор сюда не попадает.
    pub raised: Vec<RecordId>,
}

/// Применить приехавший дифф.
///
/// Работает по **транзакции**, а не по соединению: наполовину применённый дифф
/// хуже неприменённого — половина записей новая, половина старая, и понять, где
/// граница, потом нечем.
///
/// Возвращает то, что правда применилось: запись, чья секция здесь локальная,
/// не применяется вовсе, запись со старой версией не трогается (§5.2), а
/// разошедшаяся откладывается в спор и живой записи не касается (§5.5).
pub fn apply(
    tx: &Transaction<'_>,
    key: &VaultKey,
    device_id: &str,
    incoming: &[SyncRecord],
) -> CoreResult<Applied> {
    let at = now_iso();
    let mut out = Applied::default();

    for record in incoming {
        let vault = vaults::adopt(
            tx,
            &record.vault_id,
            &record.vault_name,
            &record.vault_color,
        )?;
        if !vault.sync {
            // Секция выключена здесь: содержимое локальной секции не приезжает
            // ровно так же, как не уезжает (§4.2).
            continue;
        }

        let local_version: Option<i64> = tx
            .query_row(
                "SELECT version FROM records WHERE record_id = ?1",
                [&record.record_id],
                |row| row.get(0),
            )
            .ok();

        let ancestor = ancestor(tx, device_id, &record.record_id)?;
        let take = match classify(record.version, local_version, ancestor) {
            Verdict::Take => true,
            Verdict::Keep => false,
            Verdict::Conflict => match dispute(tx, key, record)? {
                // Удаление сильнее расхождения и сильнее счётчика (§5.4).
                // Показать удалённую сторону человеку всё равно нечем — секретов
                // у надгробия нет, — а не поехавшее удаление воскресает, и это
                // худшая поломка, какая бывает у менеджера паролей.
                Dispute::RemoteDeleted => true,
                Dispute::LocalDeleted => false,
                // Обе стороны пришли к одному и тому же — спора нет, решает
                // счётчик, как решал бы и без всякого предка.
                Dispute::SameContent => remote_wins(record.version, local_version),
                Dispute::Divergence => {
                    // Живую запись не трогаем вовсе: чужая версия ложится в спор
                    // и ждёт человека. В `records` она легла бы поверх местной —
                    // то есть ровно тем тихим затиранием, от которого конфликт и
                    // защищает.
                    if conflicts::raise(
                        tx,
                        key,
                        device_id,
                        record,
                        local_version.unwrap_or_default(),
                    )? {
                        out.raised.push(record.record_id.clone());
                    }
                    false
                }
            },
        };
        if !take {
            continue;
        }

        let password_ct = records::seal_field(
            key,
            &record.record_id,
            SecretField::Password,
            record.password.as_deref(),
        )?;
        let notes_ct = records::seal_field(
            key,
            &record.record_id,
            SecretField::Notes,
            record.notes.as_deref(),
        )?;
        let totp_ct = records::seal_field(
            key,
            &record.record_id,
            SecretField::TotpSecret,
            record.totp_secret.as_deref(),
        )?;

        tx.execute(
            "INSERT INTO records (record_id, vault_id, service_name, urls, login, account_label,
                                  password_ct, notes_ct, totp_ct,
                                  version, created_at, updated_at, password_updated_at, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(record_id) DO UPDATE SET
                 vault_id = excluded.vault_id,
                 service_name = excluded.service_name,
                 urls = excluded.urls,
                 login = excluded.login,
                 account_label = excluded.account_label,
                 password_ct = excluded.password_ct,
                 notes_ct = excluded.notes_ct,
                 totp_ct = excluded.totp_ct,
                 version = excluded.version,
                 created_at = excluded.created_at,
                 updated_at = excluded.updated_at,
                 password_updated_at = excluded.password_updated_at,
                 deleted_at = excluded.deleted_at",
            rusqlite::params![
                record.record_id,
                record.vault_id,
                record.service_name,
                serde_json::to_string(&record.urls)?,
                record.login,
                record.account_label,
                password_ct,
                notes_ct,
                totp_ct,
                record.version,
                record.created_at,
                record.updated_at,
                record.password_updated_at,
                record.deleted_at,
            ],
        )?;

        // Запись переехала целиком — спорить о её версиях больше не о чем.
        conflicts::drop_for(tx, &record.record_id)?;
        out.records.push((record.record_id.clone(), record.version));
    }

    note_in(tx, device_id, &out.records, &at)?;
    Ok(out)
}

/// Что на самом деле стоит за разошедшимися счётчиками.
enum Dispute {
    /// Приехало надгробие. Удаление важнее спора (§5.4).
    RemoteDeleted,
    /// Надгробие здесь. Воскрешать запись приехавшей правкой нельзя.
    LocalDeleted,
    /// Счётчики разошлись, содержимое — нет.
    SameContent,
    /// Настоящее расхождение: обе стороны правили, и правили по-разному.
    Divergence,
}

/// Разобрать, спор перед нами или совпадение.
///
/// Два устройства сплошь и рядом приходят к одному и тому же значению одной и
/// той же правкой — например, приняв её от третьего. Поднимать на этом конфликт
/// значило бы звать человека решать спор, которого нет. Это же сравнение
/// закрывает круг после разрешения: выбранная сторона приезжает соседу с
/// содержимым, которое у него уже лежит.
fn dispute(tx: &Transaction<'_>, key: &VaultKey, record: &SyncRecord) -> CoreResult<Dispute> {
    if record.deleted_at.is_some() {
        return Ok(Dispute::RemoteDeleted);
    }
    // `local_side` отдаёт только живую запись: `None` здесь и значит надгробие.
    let Some(local) = conflicts::local_side(tx, key, &record.record_id)? else {
        return Ok(Dispute::LocalDeleted);
    };

    let remote_fields = conflicts::Fields {
        vault_id: &record.vault_id,
        service_name: &record.service_name,
        urls: &record.urls,
        login: &record.login,
        account_label: record.account_label.as_deref(),
    };
    let remote_secrets = conflicts::Secrets {
        password: record.password.clone(),
        notes: record.notes.clone(),
        totp_secret: record.totp_secret.clone(),
    };

    if conflicts::differing_fields(
        &local.fields(),
        &local.secrets,
        &remote_fields,
        &remote_secrets,
    )
    .is_empty()
    {
        Ok(Dispute::SameContent)
    } else {
        Ok(Dispute::Divergence)
    }
}

// ---------------------------------------------------------------------------
// Отметки «докуда сошлись»
// ---------------------------------------------------------------------------

/// Запомнить, на какой версии сошлись с этим устройством.
pub fn note(conn: &Connection, device_id: &str, agreed: &[(RecordId, i64)]) -> CoreResult<()> {
    note_in(conn, device_id, agreed, &now_iso())
}

fn note_in(
    conn: &Connection,
    device_id: &str,
    agreed: &[(RecordId, i64)],
    at: &IsoDateTime,
) -> CoreResult<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO sync_state (device_id, record_id, synced_version, synced_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(device_id, record_id) DO UPDATE SET
             synced_version = excluded.synced_version,
             synced_at      = excluded.synced_at",
    )?;
    for (record_id, version) in agreed {
        stmt.execute(rusqlite::params![device_id, record_id, version, at])?;
    }
    Ok(())
}

/// Забыть, что с этим устройством вообще что-то сходилось.
///
/// Зовётся при отзыве и при (повторном) сопряжении. Вернувшийся из отзыва
/// ноутбук обязан получить хранилище заново: доверять отметкам, сделанным до
/// отзыва, значит молча не довезти всё, что изменилось за это время.
pub fn forget_device(conn: &Connection, device_id: &str) -> CoreResult<()> {
    conn.execute("DELETE FROM sync_state WHERE device_id = ?1", [device_id])?;
    Ok(())
}

/// Записи, которые ещё не уехали ни на одно действующее устройство.
///
/// **Считается по пирам, а не по факту правки.** Запись «ждёт отправки», только
/// если есть кому её ждать: без сопряжённых устройств список пуст, и индикатор
/// спокойно говорит «рядом никого» вместо жёлтого «изменения ждут отправки».
/// Это то же рассуждение, которым контракт исключает отсюда локальные секции:
/// «им некуда ехать» (§4.2).
///
/// Надгробия сюда попадают наравне с живыми записями: недоехавшее удаление —
/// такое же недоехавшее изменение (§5.4).
pub fn pending(conn: &Connection, this_device_id: &str) -> CoreResult<Vec<RecordId>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT r.record_id, r.updated_at
         FROM records r
         JOIN vaults  v ON v.vault_id = r.vault_id AND v.sync = 1
         JOIN devices d ON d.device_id <> ?1 AND d.revoked_at IS NULL
         LEFT JOIN sync_state s
                ON s.device_id = d.device_id AND s.record_id = r.record_id
         WHERE s.synced_version IS NULL OR s.synced_version < r.version
         ORDER BY r.updated_at, r.record_id",
    )?;
    let ids = stmt
        .query_map([this_device_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_version_wins_and_an_older_one_does_not() {
        assert!(remote_wins(2, Some(1)));
        assert!(!remote_wins(1, Some(2)));
        // Равные счётчики сами по себе никого не двигают.
        assert!(!remote_wins(2, Some(2)));
        // Записи нет вовсе — берём любую, включая надгробие.
        assert!(remote_wins(1, None));
    }

    #[test]
    fn without_a_common_ancestor_the_counter_decides_alone() {
        // Так выглядит первая встреча и возврат после отзыва: отметок нет,
        // и объявлять расхождением каждую запись нельзя (§5.5).
        assert_eq!(classify(2, Some(1), None), Verdict::Take);
        assert_eq!(classify(1, Some(2), None), Verdict::Keep);
        assert_eq!(classify(2, Some(2), None), Verdict::Keep);
        assert_eq!(classify(1, None, None), Verdict::Take);
        // Записи нет вовсе — спорить не с чем даже при известном предке.
        assert_eq!(classify(1, None, Some(1)), Verdict::Take);
    }

    #[test]
    fn only_a_move_on_both_sides_is_a_conflict() {
        // Ушли обе — расхождение, и неважно, чей счётчик больше.
        assert_eq!(classify(5, Some(5), Some(3)), Verdict::Conflict);
        assert_eq!(classify(6, Some(5), Some(3)), Verdict::Conflict);
        assert_eq!(classify(4, Some(7), Some(3)), Verdict::Conflict);
        // Ушла одна — обычное обновление в ту или другую сторону.
        assert_eq!(classify(5, Some(3), Some(3)), Verdict::Take);
        assert_eq!(classify(3, Some(5), Some(3)), Verdict::Keep);
        // Не ушёл никто — сошлись.
        assert_eq!(classify(3, Some(3), Some(3)), Verdict::Keep);
    }

    #[test]
    fn debug_of_a_record_does_not_print_the_password() {
        let record = SyncRecord {
            record_id: "r1".to_owned(),
            vault_id: "v1".to_owned(),
            vault_name: "Личное".to_owned(),
            vault_color: "indigo".to_owned(),
            service_name: "GitHub".to_owned(),
            urls: vec!["github.com".to_owned()],
            login: "octocat".to_owned(),
            account_label: None,
            password: Some("совершенно-секретно".to_owned()),
            notes: None,
            totp_secret: None,
            version: 1,
            created_at: "2026-08-23T10:00:00.000Z".to_owned(),
            updated_at: "2026-08-23T10:00:00.000Z".to_owned(),
            password_updated_at: "2026-08-23T10:00:00.000Z".to_owned(),
            deleted_at: None,
        };

        let printed = format!("{record:?}");
        assert!(!printed.contains("совершенно-секретно"), "{printed}");
        assert!(printed.contains("<скрыт>"), "{printed}");
    }
}
