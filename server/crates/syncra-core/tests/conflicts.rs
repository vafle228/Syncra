//! Расхождения версий между двумя устройствами (S5, §5.5).
//!
//! Сети здесь нет намеренно, и это не упрощение. Конфликт по определению
//! появляется там, где два устройства какое-то время НЕ разговаривали, а стенд
//! из `net.rs` и `sync.rs` устроен ровно наоборот — узлы там сходятся сами,
//! каждые двести миллисекунд. Развести на нём две версии значило бы гоняться за
//! таймингом и получить тест, который иногда красный без причины.
//!
//! Поэтому круг обмена здесь крутится руками: те же `sync_manifest`,
//! `sync_plan`, `sync_export`, `sync_apply` и `sync_note`, что зовёт
//! `net::sync::run_round`, только по очереди и без сокета. Прецедент —
//! `sync.rs::a_second_round_over_the_same_state_moves_nothing`, где планы тоже
//! считаются по манифестам напрямую. Что круг едет по проводу целиком,
//! проверено там; здесь проверяется, что он решает.
//!
//! Мастер-пароли у сторон **разные** — как и в `sync.rs`: перешифровка секретов
//! на границе устройств действует и для отложенной версии конфликта.

mod common;

use common::{
    assert_code, draft, host, settings, trust_each_other, wait_until, Device2, MASTER_PASSWORD,
};
use syncra_core::{
    ConflictField, ConflictSide, Core, CoreErrorCode, CoreEvent, HostDevice, NodeSettings,
    RecordConflict, RecordMeta, RecordPatch, SecretField,
};

/// `model::RecordId` наружу не реэкспортируется — это просто `String`.
type RecordId = String;

const OTHER_PASSWORD: &str = "мастер-пароль-2";

// ---------------------------------------------------------------------------
// Стенд: два ядра, сопряжённые и умеющие обмениваться по команде
// ---------------------------------------------------------------------------

/// Два устройства, знающие друг друга по имени в `devices`.
struct Pair {
    a: Core,
    b: Core,
    /// Чем `b` подписан в `devices` у `a`.
    b_id: String,
    /// ...и наоборот.
    a_id: String,
}

fn peer_id(core: &Core) -> String {
    core.list_devices()
        .unwrap()
        .into_iter()
        .find(|device| !device.is_this_device)
        .expect("сосед записан в доверенные")
        .device_id
}

/// Сопрячь одну сторону с другой так, как это делает камера: код с экрана
/// `shows` подаётся в ядро `reads`.
fn pair_one_way(shows: &mut Core, reads: &mut Core) {
    shows.get_pairing_payload().expect("код сопряжения");
    let payload = shows
        .shown_pairing_payload()
        .expect("показанный код")
        .to_owned();

    let handshake = reads.submit_paired_key(&payload).expect("прочитанный код");
    reads
        .confirm_pairing(&handshake.session_id)
        .expect("подтверждение");
}

impl Pair {
    fn new() -> Self {
        let mut a = Core::in_memory(host()).expect("хранилище в памяти");
        a.init_vault(MASTER_PASSWORD).expect("init_vault");
        let mut b = Core::in_memory(HostDevice::desktop("Телефон")).expect("хранилище в памяти");
        b.init_vault(OTHER_PASSWORD).expect("init_vault");

        // Дважды — чтобы обе стороны знали ключ друг друга: в жизни это делает
        // сеть, здесь делать её работу нечем и незачем.
        pair_one_way(&mut a, &mut b);
        pair_one_way(&mut b, &mut a);

        let b_id = peer_id(&a);
        let a_id = peer_id(&b);
        Self { a, b, b_id, a_id }
    }

    /// Круг обмена целиком, инициатор — `a`.
    ///
    /// Один в один порядок `net::sync::run_round`: манифест инициатора →
    /// план отвечающего → дифф отвечающего → дифф инициатора → отметки. Второй
    /// манифест по проводу не едет и здесь тоже не считается.
    fn round(&mut self) {
        self.round_from(Direction::AtoB);
    }

    /// Тот же круг, но ведёт его `b`.
    fn round_back(&mut self) {
        self.round_from(Direction::BtoA);
    }

    fn round_from(&mut self, direction: Direction) {
        let (initiator, responder, peer_of_initiator, peer_of_responder) = match direction {
            Direction::AtoB => (&mut self.a, &mut self.b, &self.b_id, &self.a_id),
            Direction::BtoA => (&mut self.b, &mut self.a, &self.a_id, &self.b_id),
        };

        let manifest = initiator.sync_manifest().unwrap();
        let plan = responder.sync_plan(peer_of_responder, &manifest).unwrap();

        // Шаг 2: дифф отвечающего едет инициатору.
        let outgoing = responder.sync_export(&plan.send).unwrap();
        let sent: Vec<(RecordId, i64)> = outgoing
            .iter()
            .map(|record| (record.record_id.clone(), record.version))
            .collect();
        let received = initiator.sync_apply(peer_of_initiator, &outgoing).unwrap();

        // Шаг 3: инициатор отдаёт то, что попросили.
        let back = initiator.sync_export(&plan.want).unwrap();
        let back_sent: Vec<(RecordId, i64)> = back
            .iter()
            .map(|record| (record.record_id.clone(), record.version))
            .collect();
        responder.sync_apply(peer_of_responder, &back).unwrap();

        // Отметки — только после того, как круг доехал до конца.
        let mut agreed = back_sent;
        if plan.want_complete {
            agreed.extend(
                initiator
                    .sync_settled_after(
                        &plan.want,
                        &received
                            .records
                            .iter()
                            .map(|(id, _)| id.clone())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
            );
        }
        initiator.sync_note(peer_of_initiator, &agreed).unwrap();

        let mut agreed = sent;
        agreed.extend(plan.settled);
        responder.sync_note(peer_of_responder, &agreed).unwrap();
    }
}

enum Direction {
    AtoB,
    BtoA,
}

/// Развести одну запись: завести её, свести стороны и потом править порознь.
///
/// Общий предок появляется на первом круге — без него расхождения не бывает по
/// определению (§5.5), и половина этого файла проверяет именно это.
fn diverge(pair: &mut Pair) -> RecordId {
    let record_id = pair
        .a
        .create_record(&draft("GitHub", "octocat", "тайна-ноутбука"))
        .unwrap()
        .record_id;
    pair.round();
    assert!(
        find(&pair.b, &record_id).is_some(),
        "запись должна доехать до второго устройства"
    );
    record_id
}

fn find(core: &Core, record_id: &str) -> Option<RecordMeta> {
    core.list_records(None, true)
        .unwrap()
        .into_iter()
        .find(|record| record.record_id == record_id)
}

fn live(core: &Core, record_id: &str) -> RecordMeta {
    find(core, record_id).expect("запись на месте")
}

fn rename(core: &Core, record_id: &str, login: &str) -> RecordMeta {
    core.update_record(
        record_id,
        &RecordPatch {
            login: Some(login.to_owned()),
            ..RecordPatch::default()
        },
    )
    .unwrap()
}

fn repassword(core: &Core, record_id: &str, password: &str) -> RecordMeta {
    core.update_record(
        record_id,
        &RecordPatch {
            password: Some(password.to_owned()),
            ..RecordPatch::default()
        },
    )
    .unwrap()
}

fn only(conflicts: Vec<RecordConflict>) -> RecordConflict {
    assert_eq!(
        conflicts.len(),
        1,
        "ждали ровно один конфликт: {conflicts:?}"
    );
    conflicts.into_iter().next().unwrap()
}

// ---------------------------------------------------------------------------
// Обнаружение
// ---------------------------------------------------------------------------

/// Главный случай, ради которого весь шаг: счётчики совпали, содержимое — нет.
///
/// До S5 такое расхождение не двигало ничего и ни о чём не сообщало: обе
/// стороны молча оставались при своём и расходились дальше с каждой правкой.
#[test]
fn equal_versions_with_different_content_raise_a_conflict() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");
    assert_eq!(
        live(&pair.a, &record_id).version,
        live(&pair.b, &record_id).version,
        "счётчики обязаны совпасть — иначе проверяется не тот случай"
    );

    pair.round();

    // Конфликт поднимается там, куда приехала чужая версия: круг вёл `a`,
    // значит просил и получал `b`.
    let conflict = only(pair.b.list_conflicts().unwrap());
    assert_eq!(conflict.record_id, record_id);
    assert_eq!(conflict.local.login, "octocat-телефон");
    assert_eq!(conflict.remote.login, "octocat-ноутбук");
    assert_eq!(conflict.differing_fields, vec![ConflictField::Login]);

    // Живая запись при этом не тронута: спор ждёт человека, а не решается сам.
    assert_eq!(live(&pair.b, &record_id).login, "octocat-телефон");
}

/// Случай пострашнее равных счётчиков: приехавшая версия ЗАКОННО свежее, и до
/// S5 она молча затирала местную правку.
#[test]
fn a_newer_version_does_not_overwrite_a_local_edit_made_after_the_ancestor() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    // На `a` правок две, на `b` одна: версия у `a` больше, но ушли обе.
    rename(&pair.a, &record_id, "octocat-раз");
    rename(&pair.a, &record_id, "octocat-два");
    rename(&pair.b, &record_id, "octocat-телефон");
    assert!(live(&pair.a, &record_id).version > live(&pair.b, &record_id).version);

    pair.round();

    let conflict = only(pair.b.list_conflicts().unwrap());
    assert_eq!(conflict.remote.login, "octocat-два");
    assert_eq!(live(&pair.b, &record_id).login, "octocat-телефон");
}

/// Зеркальная страховка: обычное обновление конфликтом не становится.
///
/// Без неё легко написать «конфликт» так, что он срабатывает на каждой второй
/// синхронизации, и человек перестаёт его читать.
#[test]
fn an_ordinary_update_is_not_a_conflict() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    pair.round();

    assert!(pair.b.list_conflicts().unwrap().is_empty());
    assert_eq!(live(&pair.b, &record_id).login, "octocat-ноутбук");
}

/// Обе стороны подняли счётчик, но пришли к одному и тому же.
///
/// Так бывает, когда правку принесло третье устройство или человек набрал одно
/// и то же дважды. Спора здесь нет, и звать его разрешать некуда.
#[test]
fn the_same_edit_on_both_sides_is_not_a_conflict() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-общий");
    rename(&pair.b, &record_id, "octocat-общий");

    pair.round();

    assert!(pair.a.list_conflicts().unwrap().is_empty());
    assert!(pair.b.list_conflicts().unwrap().is_empty());
}

/// Без общего предка расхождения не бывает.
///
/// Первая встреча и возврат после отзыва выглядят одинаково: отметок нет.
/// Считать расхождением каждую запись значило бы встречать человека сотней
/// конфликтов там, где на деле просто новое знакомство.
#[test]
fn a_first_meeting_raises_nothing() {
    let mut pair = Pair::new();

    // Обе стороны завели свою запись и ни разу не сходились.
    pair.a
        .create_record(&draft("GitHub", "octocat", "тайна-ноутбука"))
        .unwrap();
    pair.b
        .create_record(&draft("GitHub", "octocat", "тайна-телефона"))
        .unwrap();

    pair.round();

    assert!(pair.a.list_conflicts().unwrap().is_empty());
    assert!(pair.b.list_conflicts().unwrap().is_empty());
    // Записи разные (`record_id` генерирует ядро), обе доехали — это не спор.
    assert_eq!(pair.b.list_records(None, false).unwrap().len(), 2);
}

/// Надгробие в споре не участвует: `ConflictVersion` не умеет сказать «эта
/// сторона удалена», а удаление важнее расхождения (§5.4).
#[test]
fn a_tombstone_wins_instead_of_arguing() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    pair.a.delete_record(&record_id).unwrap();
    rename(&pair.b, &record_id, "octocat-телефон");

    pair.round();

    assert!(pair.b.list_conflicts().unwrap().is_empty());
    // Удаление поехало дальше и запись не воскресла — §5.4 в силе.
    assert!(live(&pair.b, &record_id).deleted_at.is_some());
}

/// Один и тот же спор не поднимается событием каждую минуту.
///
/// Круги идут по расписанию, а конфликт ждёт сколько нужно: второе
/// `conflict_raised` про то же самое было бы шумом, от которого человек
/// научится отмахиваться.
#[test]
fn the_same_dispute_is_raised_once() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");

    pair.round();
    let first = only(pair.b.list_conflicts().unwrap()).raised_at;

    // Ещё три круга, в обе стороны: спор от этого не размножается и не исчезает.
    pair.round();
    pair.round_back();
    pair.round();

    let conflicts = pair.b.list_conflicts().unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        conflicts[0].raised_at, first,
        "спор тот же — и строка та же"
    );

    // И, что важнее, за эти круги никто не объявил стороны сошедшимися: живые
    // записи всё ещё разные. Одна отметка «сошлись» — и расхождение стало бы
    // неотличимо от сходимости навсегда.
    assert_eq!(live(&pair.b, &record_id).login, "octocat-телефон");
    assert_eq!(live(&pair.a, &record_id).login, "octocat-ноутбук");
}

// ---------------------------------------------------------------------------
// Что видно человеку
// ---------------------------------------------------------------------------

/// ЗАКОН №1: список конфликтов не несёт ни одного секретного значения.
#[test]
fn the_list_carries_no_secret_values() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    repassword(&pair.a, &record_id, "пароль-ноутбука");
    repassword(&pair.b, &record_id, "пароль-телефона");
    pair.round();

    let conflicts = pair.b.list_conflicts().unwrap();
    let wire = serde_json::to_string(&conflicts).unwrap();

    assert!(!wire.contains("пароль-ноутбука"), "{wire}");
    assert!(!wire.contains("пароль-телефона"), "{wire}");
    assert!(!wire.contains("тайна-ноутбука"), "{wire}");
    // При этом ИМЯ поля в списке есть: «пароли различаются» — факт о записи.
    assert_eq!(conflicts[0].differing_fields, vec![ConflictField::Password]);
}

/// Стороны подписаны именами устройств — «какая версия чья» (§5.5).
#[test]
fn each_side_is_signed_with_its_device_name() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();

    let conflict = only(pair.b.list_conflicts().unwrap());
    assert_eq!(conflict.local.side, ConflictSide::Local);
    assert_eq!(conflict.local.device_name, "Телефон");
    assert_eq!(conflict.remote.side, ConflictSide::Remote);
    assert_eq!(conflict.remote.device_name, common::HOST_NAME);
}

/// Местная сторона — живая запись, а не снимок момента подъёма.
///
/// Пока спор ждёт, запись можно править, и показывать человеку устаревший
/// вариант его же данных было бы враньём.
#[test]
fn the_local_side_follows_the_record_while_the_dispute_waits() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();

    let before = only(pair.b.list_conflicts().unwrap());
    assert_eq!(before.differing_fields, vec![ConflictField::Login]);

    // Правка уже ПОСЛЕ подъёма спора.
    repassword(&pair.b, &record_id, "новый-пароль");

    let after = only(pair.b.list_conflicts().unwrap());
    assert_eq!(after.raised_at, before.raised_at, "момент подъёма — факт");
    assert!(after.local.version > before.local.version);
    assert_eq!(
        after.differing_fields,
        vec![ConflictField::Login, ConflictField::Password]
    );
}

/// Удалили запись — спорить о её версиях не о чем.
#[test]
fn deleting_the_record_drops_the_dispute() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();
    assert_eq!(pair.b.list_conflicts().unwrap().len(), 1);

    pair.b.delete_record(&record_id).unwrap();
    assert!(pair.b.list_conflicts().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Открыть одно поле обеих версий
// ---------------------------------------------------------------------------

#[test]
fn a_secret_of_both_sides_opens_at_once() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    repassword(&pair.a, &record_id, "пароль-ноутбука");
    repassword(&pair.b, &record_id, "пароль-телефона");
    pair.round();

    // Пароль приехал через границу устройств с РАЗНЫМИ мастер-паролями и
    // читается — значит, его перешифровали местным ключом, а не скопировали
    // шифротекстом (§3.2).
    let secrets = pair
        .b
        .conflict_secret(&record_id, SecretField::Password)
        .unwrap();
    assert_eq!(secrets.local.as_deref(), Some("пароль-телефона"));
    assert_eq!(secrets.remote.as_deref(), Some("пароль-ноутбука"));

    // Незаполненное поле — `null` с обеих сторон, а не ошибка.
    let notes = pair
        .b
        .conflict_secret(&record_id, SecretField::Notes)
        .unwrap();
    assert_eq!(notes.local, None);
    assert_eq!(notes.remote, None);
}

#[test]
fn a_record_without_a_dispute_has_no_conflict_secret() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    assert_code(
        pair.b.conflict_secret(&record_id, SecretField::Password),
        CoreErrorCode::NotFound,
    );
}

#[test]
fn conflicts_stay_behind_the_lock() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();

    pair.b.lock();
    assert_code(pair.b.list_conflicts(), CoreErrorCode::Locked);
    assert_code(
        pair.b.conflict_secret(&record_id, SecretField::Password),
        CoreErrorCode::Locked,
    );
    assert_code(
        pair.b.resolve_conflict(&record_id, ConflictSide::Local),
        CoreErrorCode::Locked,
    );
}

// ---------------------------------------------------------------------------
// Разрешение
// ---------------------------------------------------------------------------

#[test]
fn keeping_the_local_side_bumps_the_version_over_both() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.a, &record_id, "octocat-два");
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();

    let conflict = only(pair.b.list_conflicts().unwrap());
    let expected = conflict.local.version.max(conflict.remote.version) + 1;

    let resolved = pair
        .b
        .resolve_conflict(&record_id, ConflictSide::Local)
        .unwrap();

    assert_eq!(resolved.login, "octocat-телефон");
    // Не `местная + 1`: с ним проигравшая вернулась бы следующим кругом как
    // более свежая, и человек выбирал бы заново до бесконечности.
    assert_eq!(resolved.version, expected);
    assert!(pair.b.list_conflicts().unwrap().is_empty());
}

#[test]
fn taking_the_remote_side_moves_metadata_and_secrets() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    pair.a
        .update_record(
            &record_id,
            &RecordPatch {
                login: Some("octocat-ноутбук".to_owned()),
                password: Some("пароль-ноутбука".to_owned()),
                notes: Some(Some("вторая почта".to_owned())),
                ..RecordPatch::default()
            },
        )
        .unwrap();
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();

    let before = live(&pair.b, &record_id);
    let resolved = pair
        .b
        .resolve_conflict(&record_id, ConflictSide::Remote)
        .unwrap();

    assert_eq!(resolved.login, "octocat-ноутбук");
    assert!(resolved.has_notes);
    assert_eq!(
        pair.b.get_secret(&record_id).unwrap().password,
        "пароль-ноутбука"
    );
    // Пароль правда сменился — значит сменилась и дата его смены (§4.1).
    assert_eq!(resolved.password_updated_at, resolved.updated_at);
    assert_ne!(resolved.password_updated_at, before.password_updated_at);
}

/// `password_updated_at` — это «когда пароль правда сменили», а не «когда
/// запись трогали». Победившая версия с тем же паролем его не двигает.
#[test]
fn an_unchanged_password_keeps_its_date() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    // Расходятся только логины; пароль у обеих сторон прежний.
    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();

    let before = live(&pair.b, &record_id);
    let resolved = pair
        .b
        .resolve_conflict(&record_id, ConflictSide::Remote)
        .unwrap();

    assert_eq!(resolved.login, "octocat-ноутбук");
    assert_eq!(resolved.password_updated_at, before.password_updated_at);
}

#[test]
fn resolving_twice_is_a_not_found() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();

    pair.b
        .resolve_conflict(&record_id, ConflictSide::Local)
        .unwrap();
    assert_code(
        pair.b.resolve_conflict(&record_id, ConflictSide::Local),
        CoreErrorCode::NotFound,
    );
    assert_code(
        pair.b.conflict_secret(&record_id, SecretField::Password),
        CoreErrorCode::NotFound,
    );
}

/// Самый ценный тест файла: выбор человека уезжает соседу и НЕ поднимает там
/// спор заново.
///
/// Без сдвига общего предка при разрешении вышло бы кольцо: победившая версия
/// больше предка, чужая тоже больше предка — и сосед честно поднял бы тот же
/// конфликт, который здесь только что закрыли.
#[test]
fn a_resolved_side_travels_and_does_not_raise_a_second_dispute() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();

    // Человек сидит за `b` и оставляет своё.
    pair.b
        .resolve_conflict(&record_id, ConflictSide::Local)
        .unwrap();

    // Следующий круг — и `a` принимает выбор молча.
    pair.round();

    assert_eq!(live(&pair.a, &record_id).login, "octocat-телефон");
    assert!(
        pair.a.list_conflicts().unwrap().is_empty(),
        "выбор человека не должен всплывать спором на втором устройстве"
    );
    assert!(pair.b.list_conflicts().unwrap().is_empty());
    assert_eq!(
        live(&pair.a, &record_id).version,
        live(&pair.b, &record_id).version
    );
}

/// То же, но круг ведёт вторая сторона: спор поднимается у того, кто получает,
/// кем бы он ни был в этом круге.
#[test]
fn the_dispute_lands_on_whoever_receives() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");

    pair.round_back();

    assert_eq!(pair.a.list_conflicts().unwrap().len(), 1);
    assert!(pair.b.list_conflicts().unwrap().is_empty());
}

/// После разрешения запись снова ждёт отправки: выбор — такое же изменение,
/// и его ещё надо довезти (`mock/index.ts:1395`).
#[test]
fn a_resolved_record_is_pending_again() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    rename(&pair.a, &record_id, "octocat-ноутбук");
    rename(&pair.b, &record_id, "octocat-телефон");
    pair.round();

    pair.b
        .resolve_conflict(&record_id, ConflictSide::Local)
        .unwrap();

    assert_eq!(pair.b.pending_records().unwrap(), vec![record_id.clone()]);
    pair.round_back();
    assert!(pair.b.pending_records().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Хранилище
// ---------------------------------------------------------------------------

/// Отложенная версия защищена ключом хранилища наравне с паролями — значит,
/// смена мастер-пароля обязана перешифровать и её.
///
/// Пропустить эту таблицу — значит потерять приехавшую сторону спора и узнать
/// об этом только на экране разрешения, через неделю.
#[test]
fn the_pending_version_survives_a_master_password_change() {
    let mut pair = Pair::new();
    let record_id = diverge(&mut pair);

    repassword(&pair.a, &record_id, "пароль-ноутбука");
    repassword(&pair.b, &record_id, "пароль-телефона");
    pair.round();

    pair.b
        .change_master_password(OTHER_PASSWORD, "мастер-пароль-3")
        .unwrap();

    let secrets = pair
        .b
        .conflict_secret(&record_id, SecretField::Password)
        .unwrap();
    assert_eq!(secrets.local.as_deref(), Some("пароль-телефона"));
    assert_eq!(secrets.remote.as_deref(), Some("пароль-ноутбука"));
}

/// Хранилище схемы 3 создавалось без таблицы `conflicts`.
///
/// Открытие таким хранилищем не должно ни падать, ни терять записи: таблица
/// досоздаётся при первом же открытии, а споров в ней справедливо ноль.
#[test]
fn a_schema_3_vault_migrates_without_losing_anything() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syncra.db");

    let record_id = {
        let mut core = Core::open(&path, host()).unwrap();
        core.init_vault(MASTER_PASSWORD).unwrap();
        core.create_record(&draft("GitHub", "octocat", "тайна-1"))
            .unwrap()
            .record_id
    };

    // Откатываем файл до схемы 3 мимо ядра — ровно то, что лежит на диске у
    // человека, поставившего прошлую версию: таблицы споров ещё нет, метаданные
    // записи лежат открыто.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE conflicts;").unwrap();
        common::unseal_metadata(&conn, "records");
        conn.execute_batch(
            "UPDATE records SET service_name = 'GitHub', urls = '[\"github.com\"]',
                                login = 'octocat';
             UPDATE meta SET value = CAST('3' AS BLOB) WHERE key = 'schema_version';",
        )
        .unwrap();
    }

    let mut core = Core::open(&path, host()).unwrap();
    core.unlock(MASTER_PASSWORD).unwrap();

    assert_eq!(core.list_records(None, false).unwrap().len(), 1);
    assert_eq!(
        core.list_records(None, false).unwrap()[0].service_name,
        "GitHub"
    );
    assert_eq!(core.get_secret(&record_id).unwrap().password, "тайна-1");
    assert!(core.list_conflicts().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// По проводу: событие
// ---------------------------------------------------------------------------

/// Расхождение, поднятое настоящим кругом по TCP, уходит наружу событием.
///
/// Стенд тот же, что у `sync.rs`, но с выключенным автоциклом: круги здесь
/// гоняются по `sync_now`, иначе развести две версии не успеть — узлы сходятся
/// сами каждые двести миллисекунд.
#[test]
fn a_dispute_over_the_wire_announces_itself() {
    let quiet = NodeSettings {
        // Час — это «никогда» для теста: круг пойдёт только по кнопке.
        sync_interval: std::time::Duration::from_secs(3600),
        ..settings()
    };
    let a = Device2::with_settings("Ноутбук", MASTER_PASSWORD, quiet.clone());
    let b = Device2::with_settings("Телефон", OTHER_PASSWORD, quiet);
    trust_each_other(&a, &b);

    let record_id = a
        .with_core(|core| {
            core.create_record(&draft("GitHub", "octocat", "тайна"))
                .unwrap()
        })
        .record_id;

    a.node.sync_now();
    wait_until("запись доехала", || {
        b.with_core(|core| core.list_records(None, true).unwrap())
            .iter()
            .any(|record| record.record_id == record_id)
    });

    a.with_core(|core| rename(core, &record_id, "octocat-ноутбук"));
    b.with_core(|core| rename(core, &record_id, "octocat-телефон"));

    a.node.sync_now();

    let event = b.wait_event(|event| matches!(event, CoreEvent::ConflictRaised(_)));
    let CoreEvent::ConflictRaised(conflict) = event else {
        unreachable!("отфильтровали выше")
    };
    assert_eq!(conflict.record_id, record_id);
    assert_eq!(conflict.differing_fields, vec![ConflictField::Login]);
    assert_eq!(conflict.remote.login, "octocat-ноутбук");
    assert_eq!(conflict.local.login, "octocat-телефон");

    // Событие несёт конфликт целиком (`contract.ts:1324`) — то же, что отдал бы
    // `list_conflicts`.
    let listed = only(b.with_core(|core| core.list_conflicts().unwrap()));
    assert_eq!(listed.record_id, conflict.record_id);
}

/// Незнакомое имя поля — это `VALIDATION` с человеческим текстом, а не провал
/// разбора запроса: из провала фронт слепил бы «непредвиденную ошибку ядра».
#[test]
fn an_unknown_secret_field_is_a_validation_error() {
    assert_code(
        SecretField::parse("recovery_codes"),
        CoreErrorCode::Validation,
    );
    assert_code(ConflictSide::parse("theirs"), CoreErrorCode::Validation);

    assert_eq!(
        SecretField::parse("password").unwrap(),
        SecretField::Password
    );
    assert_eq!(ConflictSide::parse("remote").unwrap(), ConflictSide::Remote);
}
