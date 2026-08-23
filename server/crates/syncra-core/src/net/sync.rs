//! Круг обмена записями по проводу (F10, §5.3).
//!
//! Что именно едет — дело [`crate::sync`]. Здесь только как: чередование
//! реплик, нарезка на порции и счёт перенесённого.
//!
//! # Почему строгое чередование
//!
//! Отвечающая сторона — это цикл «принял кадр → ответил кадром»
//! (`super::serve_trusted`). Своей инициативы у неё нет и быть не может: два
//! кадра подряд в одну сторону — это или дедлок, или рассинхрон счётчика nonce
//! в канале. Поэтому круг ведёт инициатор, и на каждый его запрос приходит
//! ровно один ответ:
//!
//! ```text
//! инициатор                             отвечающий
//!   SyncManifest{…, last: false}   →     SyncAck
//!   SyncManifest{…, last: true}    →     SyncNeed{wanted}   ← и отложил свой дифф
//!   SyncFetch                      →     SyncBatch{…, last: false}
//!   SyncFetch                      →     SyncBatch{…, last: true}
//!   SyncBatch{…, last: false}      →     SyncAck
//!   SyncBatch{…, last: true}       →     SyncDone{applied}
//! ```
//!
//! Обе стороны считают по одному и тому же правилу ([`crate::sync::plan`]),
//! поэтому «что я пошлю» у одной совпадает с «что я попрошу» у другой —
//! договариваться об этом отдельно не нужно, и второй манифест по проводу не
//! едет.
//!
//! # Почему порциями
//!
//! Кадр ограничен мегабайтом ([`super::wire::MAX_FRAME`]), и потолок этот
//! опустить нельзя: длину кадра называет сторона на том конце. Хранилище на
//! тысячу записей в один кадр не влезает, поэтому и манифест, и дифф едут
//! порциями, а `last` закрывает поток.

use crate::error::{CoreError, CoreResult};
use crate::model::RecordId;
use crate::session::Core;
use crate::sync::{self, SyncRecord};

use super::wire::Msg;
use super::{handshake::Established, Context, CoreEvent};

/// Единственный способ дотронуться до ядра отсюда — и он же единственный
/// способ не задержать замок дольше одной короткой операции.
fn with_core<T>(
    context: &Context,
    action: impl FnOnce(&mut Core) -> CoreResult<T>,
) -> CoreResult<T> {
    context
        .with_core(action)
        .unwrap_or_else(|| Err(CoreError::locked()))
}

/// Собеседник ответил не тем, чего ждали в этом месте разговора.
fn out_of_turn() -> CoreError {
    super::wire::malformed()
}

/// Применить порцию и рассказать о поднятых расхождениях.
///
/// Событие собирается и уходит ПОСЛЕ того, как замок отпущен: держать мьютекс
/// ядра на время рассылки нельзя — так же устроены `peer_found` и `sync_status`.
/// Каждое `conflict_raised` несёт готовый `RecordConflict`, как обещает
/// `contract.ts:1324`.
fn apply_batch(
    context: &Context,
    device_id: &str,
    records: &[SyncRecord],
) -> CoreResult<Vec<(RecordId, i64)>> {
    let applied = with_core(context, |core| core.sync_apply(device_id, records))?;

    for record_id in &applied.raised {
        // Запись успела исчезнуть, пока событие собиралось, — рассказывать не о
        // чем. Это не ошибка круга: круг уже сделал своё дело.
        if let Some(conflict) = with_core(context, |core| core.build_conflict(record_id))? {
            context
                .shared
                .announce(CoreEvent::ConflictRaised(Box::new(conflict)));
        }
    }

    Ok(applied.records)
}

// ---------------------------------------------------------------------------
// Сторона инициатора
// ---------------------------------------------------------------------------

/// Провести круг обмена с устройством и вернуть, сколько записей переехало.
///
/// Считается движение в обе стороны: и то, что легло здесь, и то, что легло у
/// собеседника. Для человека это один обмен, а не два.
///
/// `None` означает «собеседник сейчас занят встречным кругом». Это не ошибка и
/// показывать её человеку нечего: через минуту (или по кнопке) круг пройдёт.
pub(super) fn run_round(
    context: &Context,
    session: &mut Established,
    device_id: &str,
) -> CoreResult<Option<u32>> {
    let Some(wanted) = send_manifest(context, session)? else {
        return Ok(None);
    };
    let (applied_here, received) = pull(context, session, device_id)?;
    let (applied_there, sent) = push(context, session, &wanted.records)?;

    // Отметки ставятся ПОСЛЕ того, как круг доехал до конца: круг, оборвавшийся
    // на середине, не даёт права считать что-либо доехавшим.
    let mut agreed = sent;
    if wanted.complete {
        agreed.extend(with_core(context, |core| {
            core.sync_settled_after(&wanted.records, &received)
        })?);
    }
    with_core(context, |core| core.sync_note(device_id, &agreed))?;
    with_core(context, |core| core.sync_finish())?;

    Ok(Some(applied_here + applied_there))
}

struct Wanted {
    records: Vec<RecordId>,
    complete: bool,
}

/// Шаг 1: рассказать о своём состоянии и узнать, что из этого просят.
fn send_manifest(context: &Context, session: &mut Established) -> CoreResult<Option<Wanted>> {
    let manifest = with_core(context, |core| core.sync_manifest())?;

    // Пустой манифест — тоже манифест: «у меня ничего нет» собеседник обязан
    // услышать, иначе он не поймёт, что пора отдавать своё.
    let chunks = manifest.entries.chunks(sync::MANIFEST_BATCH).count().max(1);
    let mut entries = manifest.entries.chunks(sync::MANIFEST_BATCH);

    for index in 0..chunks {
        let last = index + 1 == chunks;
        let message = Msg::SyncManifest {
            entries: entries.next().unwrap_or_default().to_vec(),
            local_vaults: manifest.local_vaults.clone(),
            last,
        };
        match (session.channel.request(&message)?, last) {
            (Msg::SyncAck, false) => {}
            (Msg::SyncNeed { wanted, complete }, true) => {
                return Ok(Some(Wanted {
                    records: wanted,
                    complete,
                }))
            }
            // Занят встречным кругом — расходимся молча.
            (Msg::SyncBusy, _) => return Ok(None),
            _ => return Err(out_of_turn()),
        }
    }

    Err(out_of_turn())
}

/// Шаг 2: забрать дифф собеседника.
fn pull(
    context: &Context,
    session: &mut Established,
    device_id: &str,
) -> CoreResult<(u32, Vec<RecordId>)> {
    let mut applied = 0;
    let mut received = Vec::new();

    loop {
        let Msg::SyncBatch { records, last } = session.channel.request(&Msg::SyncFetch)? else {
            return Err(out_of_turn());
        };
        let laid = apply_batch(context, device_id, &records)?;
        applied += laid.len() as u32;
        received.extend(laid.into_iter().map(|(record_id, _)| record_id));

        if last {
            return Ok((applied, received));
        }
    }
}

/// Шаг 3: отдать то, что попросили.
fn push(
    context: &Context,
    session: &mut Established,
    wanted: &[RecordId],
) -> CoreResult<(u32, Vec<(RecordId, i64)>)> {
    let outgoing = with_core(context, |core| core.sync_export(wanted))?;
    let sent = outgoing
        .iter()
        .map(|record| (record.record_id.clone(), record.version))
        .collect();

    let batches = split(outgoing);
    let count = batches.len();
    for (index, records) in batches.into_iter().enumerate() {
        let last = index + 1 == count;
        match (
            session.channel.request(&Msg::SyncBatch { records, last })?,
            last,
        ) {
            (Msg::SyncAck, false) => {}
            (Msg::SyncDone { applied }, true) => return Ok((applied, sent)),
            _ => return Err(out_of_turn()),
        }
    }

    Err(out_of_turn())
}

// ---------------------------------------------------------------------------
// Сторона отвечающего
// ---------------------------------------------------------------------------

/// Состояние круга у отвечающей стороны.
///
/// Живёт локальной переменной обслуживающего потока, а не полем `Core`: круг
/// принадлежит соединению и умирает вместе с ним. Класть его в ядро значило бы
/// дать одному соседу испортить круг другому.
#[derive(Default)]
pub(super) struct Round {
    manifest: sync::Manifest,
    /// Дифф, отложенный для собеседника после закрытия манифеста.
    outgoing: Vec<Vec<SyncRecord>>,
    handed: usize,
    sent: Vec<(RecordId, i64)>,
    settled: Vec<(RecordId, i64)>,
    applied: u32,
}

/// Ответить на один кадр обмена. `true` во втором члене — круг закрыт.
pub(super) fn answer(
    context: &Context,
    round: &mut Round,
    device_id: &str,
    message: Msg,
) -> CoreResult<(Msg, bool)> {
    match message {
        Msg::SyncManifest {
            entries,
            local_vaults,
            last,
        } => {
            round.manifest.entries.extend(entries);
            round.manifest.local_vaults = local_vaults;
            if !last {
                return Ok((Msg::SyncAck, false));
            }

            let plan = with_core(context, |core| core.sync_plan(device_id, &round.manifest))?;
            let outgoing = with_core(context, |core| core.sync_export(&plan.send))?;
            round.sent = outgoing
                .iter()
                .map(|record| (record.record_id.clone(), record.version))
                .collect();
            round.outgoing = split(outgoing);
            round.settled = plan.settled;

            Ok((
                Msg::SyncNeed {
                    wanted: plan.want,
                    complete: plan.want_complete,
                },
                false,
            ))
        }

        Msg::SyncFetch => {
            let records = round
                .outgoing
                .get(round.handed)
                .cloned()
                .unwrap_or_default();
            round.handed += 1;
            let last = round.handed >= round.outgoing.len();
            Ok((Msg::SyncBatch { records, last }, false))
        }

        Msg::SyncBatch { records, last } => {
            let laid = apply_batch(context, device_id, &records)?;
            round.applied += laid.len() as u32;
            if !last {
                return Ok((Msg::SyncAck, false));
            }

            // Круг доехал до конца — только теперь отметки становятся правдой.
            let mut agreed = std::mem::take(&mut round.sent);
            agreed.append(&mut round.settled);
            with_core(context, |core| core.sync_note(device_id, &agreed))?;
            with_core(context, |core| core.sync_finish())?;

            Ok((
                Msg::SyncDone {
                    applied: round.applied,
                },
                true,
            ))
        }

        _ => Err(out_of_turn()),
    }
}

// ---------------------------------------------------------------------------
// Нарезка
// ---------------------------------------------------------------------------

/// Разложить дифф по порциям: по счёту и по весу, что кончится раньше.
///
/// Всегда хотя бы одна порция — пустая тоже кадр: без неё собеседник не узнает,
/// что отдавать больше нечего, и будет спрашивать вечно.
///
/// Одинокая запись, которая не влезает в кадр даже в одиночку, уедет всё равно:
/// круг на ней честно оборвётся и повторится. Молча выбросить её нельзя — это
/// была бы навсегда не доехавшая запись, о которой никто не узнал.
fn split(records: Vec<SyncRecord>) -> Vec<Vec<SyncRecord>> {
    let mut batches: Vec<Vec<SyncRecord>> = Vec::new();
    let mut batch: Vec<SyncRecord> = Vec::new();
    let mut weight = 0;

    for record in records {
        let own = record_weight(&record);
        if !batch.is_empty()
            && (batch.len() >= sync::RECORD_BATCH || weight + own > sync::RECORD_BATCH_BYTES)
        {
            batches.push(std::mem::take(&mut batch));
            weight = 0;
        }
        weight += own;
        batch.push(record);
    }

    batches.push(batch);
    batches
}

/// Во что запись обойдётся кадру. Оценка сверху и грубая: точный размер даёт
/// только кодирование, а гонять его дважды ради порции незачем.
fn record_weight(record: &SyncRecord) -> usize {
    const OVERHEAD: usize = 256;

    let optional = |value: &Option<String>| value.as_ref().map_or(0, String::len);
    OVERHEAD
        + record.record_id.len()
        + record.vault_id.len()
        + record.vault_name.len()
        + record.service_name.len()
        + record.login.len()
        + record.urls.iter().map(|url| url.len() + 4).sum::<usize>()
        + optional(&record.account_label)
        + optional(&record.password)
        + optional(&record.notes)
        + optional(&record.totp_secret)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn ids(batch: &[SyncRecord]) -> HashSet<&str> {
        batch
            .iter()
            .map(|record| record.record_id.as_str())
            .collect()
    }

    fn record(id: &str, notes: Option<String>) -> SyncRecord {
        SyncRecord {
            record_id: id.to_owned(),
            vault_id: "v".to_owned(),
            vault_name: "Личное".to_owned(),
            vault_color: "indigo".to_owned(),
            service_name: "S".to_owned(),
            urls: Vec::new(),
            login: "l".to_owned(),
            account_label: None,
            password: Some("p".to_owned()),
            notes,
            totp_secret: None,
            version: 1,
            created_at: "2026-08-23T10:00:00.000Z".to_owned(),
            updated_at: "2026-08-23T10:00:00.000Z".to_owned(),
            password_updated_at: "2026-08-23T10:00:00.000Z".to_owned(),
            deleted_at: None,
        }
    }

    #[test]
    fn an_empty_diff_is_still_one_batch() {
        let batches = split(Vec::new());
        assert_eq!(batches.len(), 1);
        assert!(batches[0].is_empty());
    }

    #[test]
    fn batches_close_on_count() {
        let records = (0..sync::RECORD_BATCH * 2 + 1)
            .map(|index| record(&format!("r{index}"), None))
            .collect();
        let batches = split(records);

        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), sync::RECORD_BATCH);
        assert_eq!(batches[2].len(), 1);
    }

    #[test]
    fn a_heavy_record_closes_the_batch_early() {
        let heavy = "я".repeat(sync::RECORD_BATCH_BYTES);
        let batches = split(vec![
            record("light", None),
            record("heavy", Some(heavy)),
            record("after", None),
        ]);

        // Тяжёлая едет одна, лёгкие — до и после неё. Ни одна не потерялась.
        assert_eq!(batches.len(), 3);
        assert_eq!(ids(&batches[0]), HashSet::from(["light"]));
        assert_eq!(ids(&batches[1]), HashSet::from(["heavy"]));
        assert_eq!(ids(&batches[2]), HashSet::from(["after"]));
    }
}
