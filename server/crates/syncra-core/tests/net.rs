//! Транспорт и обнаружение (S3, §5.1 · §5.6).
//!
//! «Второе устройство» здесь — второй узел в том же процессе, на своём
//! хранилище и со своим мастер-паролем; разговаривают они через настоящий TCP
//! на loopback. Это ровно то, что происходит в жизни, минус километр провода.
//!
//! **mDNS в этих тестах выключен.** Настоящий мультикаст зависит от
//! брандмауэра, от виртуальных адаптеров и от того, что ещё крутится в сети у
//! запускающего, — то есть проверял бы не ядро. Адреса приходят узлу готовыми
//! (`seed_peer`), как приходил бы результат поиска. Сам поиск проверяется
//! `#[ignore]`-тестом в конце файла, который гоняют руками.

mod common;

use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{assert_code, MASTER_PASSWORD};
use syncra_core::net::wire::{Mode, Msg};
use syncra_core::net::{handshake, CoreEvent};
use syncra_core::{
    Core, CoreErrorCode, Device, DeviceKind, HostDevice, Node, NodeSettings, SyncPhase,
};

// ---------------------------------------------------------------------------
// Стенд
// ---------------------------------------------------------------------------

/// Сроки, ужатые до тестовых: ждать по пятнадцать секунд здесь нечего.
fn settings() -> NodeSettings {
    NodeSettings {
        discovery: false,
        port: 0,
        io_timeout: Duration::from_secs(3),
        connect_timeout: Duration::from_millis(800),
        probe_interval: Duration::from_millis(150),
        peer_ttl: Duration::from_millis(600),
        stranger_backoff: Duration::from_millis(200),
        search_window: Duration::from_millis(300),
    }
}

/// Устройство целиком: ядро, узел и очередь его событий.
struct Device2 {
    core: Arc<Mutex<Core>>,
    node: Node,
    events: Receiver<CoreEvent>,
}

impl Device2 {
    fn new(name: &str, master_password: &str) -> Self {
        Self::with_settings(name, master_password, settings())
    }

    fn with_settings(name: &str, master_password: &str, settings: NodeSettings) -> Self {
        let mut core = Core::in_memory(HostDevice::desktop(name)).expect("хранилище в памяти");
        core.init_vault(master_password).expect("init_vault");

        let core = Arc::new(Mutex::new(core));
        let (node, events) = Node::new(Arc::clone(&core), settings);
        node.tick();
        Self { core, node, events }
    }

    fn addr(&self) -> SocketAddr {
        self.node.local_addr().expect("узел слушает порт")
    }

    fn with_core<T>(&self, action: impl FnOnce(&mut Core) -> T) -> T {
        action(&mut self.core.lock().unwrap())
    }

    fn peers(&self) -> Vec<Device> {
        self.with_core(|core| core.list_devices().unwrap())
            .into_iter()
            .filter(|device| !device.is_this_device)
            .collect()
    }

    /// Дождаться события, подходящего под условие. Дольше секунды здесь ничего
    /// не происходит — а если происходит, тест обязан упасть, а не висеть.
    fn wait_event(&self, matches: impl Fn(&CoreEvent) -> bool) -> CoreEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(event) = self.events.recv_timeout(Duration::from_millis(100)) {
                if matches(&event) {
                    return event;
                }
            }
        }
        panic!("события так и не дождались");
    }
}

/// Свести два устройства так, как это сделал бы человек с камерой: код с одного
/// экрана подаётся в другое ядро. Возвращает обе стороны уже доверенными.
fn pair(shows: &Device2, reads: &Device2) {
    let payload = shows.with_core(|core| {
        core.get_pairing_payload().expect("код сопряжения");
        core.shown_pairing_payload()
            .expect("показанный код")
            .to_owned()
    });

    let handshake = reads
        .with_core(|core| core.submit_paired_key(&payload))
        .expect("прочитанный код");
    reads
        .with_core(|core| core.confirm_pairing(&handshake.session_id))
        .expect("подтверждение");

    // Показавшая сторона узнаёт об этом по сети — как и в жизни.
    shows.node.seed_peer(reads.addr());
    reads.node.seed_peer(shows.addr());
}

/// Записать соседа в доверенные напрямую, минуя сеть: так проще готовить стенд
/// там, где проверяется не сопряжение, а рукопожатие.
fn trust_each_other(a: &Device2, b: &Device2) {
    for (left, right) in [(a, b), (b, a)] {
        let payload = right.with_core(|core| {
            core.get_pairing_payload().unwrap();
            core.shown_pairing_payload().unwrap().to_owned()
        });
        let handshake = left
            .with_core(|core| core.submit_paired_key(&payload))
            .unwrap();
        left.with_core(|core| core.confirm_pairing(&handshake.session_id))
            .unwrap();
    }
    a.node.seed_peer(b.addr());
    b.node.seed_peer(a.addr());
}

// ---------------------------------------------------------------------------
// Рукопожатие
// ---------------------------------------------------------------------------

#[test]
fn two_paired_cores_shake_hands_on_loopback() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", "мастер-пароль-2");
    trust_each_other(&a, &b);

    // Обходчик A звонит B, B отвечает, и оба это замечают.
    let found = a.wait_event(|event| matches!(event, CoreEvent::PeerFound(_)));
    let CoreEvent::PeerFound(peer) = found else {
        unreachable!()
    };
    assert_eq!(peer.name, "Телефон");
    assert_eq!(peer.kind, DeviceKind::Desktop);

    assert_eq!(a.node.status().peers_online, 1);
}

#[test]
fn an_unknown_key_does_not_pass() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    // Чужак поднят, но никто никого не сопрягал: в `devices` друг друга нет.
    let stranger = Device2::new("Чужой", "мастер-пароль-3");

    let identity = stranger.with_core(|core| core.net_identity().unwrap());
    let session = dial(a.addr(), &identity, Mode::Trusted);

    // Рукопожатие как таковое проходит — ключи настоящие с обеих сторон, — но
    // разговора не выходит: доверие решается по `devices`, и там пусто.
    let mut session = session.expect("рукопожатие состоялось");
    assert!(session.channel.request(&Msg::Ping).is_err());

    assert_eq!(a.node.status().peers_online, 0);
    assert!(a.peers().is_empty());
}

#[test]
fn a_revoked_device_does_not_pass() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", "мастер-пароль-2");
    trust_each_other(&a, &b);

    // Сначала убедимся, что до отзыва разговор шёл.
    a.wait_event(|event| matches!(event, CoreEvent::PeerFound(_)));

    let peer_id = a.peers()[0].device_id.clone();
    a.with_core(|core| core.revoke_device(&peer_id)).unwrap();

    // Ключ тот же, устройство то же, адрес тот же — и всё равно мимо (§2.3).
    let identity = b.with_core(|core| core.net_identity().unwrap());
    let mut session = dial(a.addr(), &identity, Mode::Trusted).expect("рукопожатие");
    assert!(session.channel.request(&Msg::Ping).is_err());
}

#[test]
fn a_forged_signature_does_not_pass() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", "мастер-пароль-2");
    trust_each_other(&a, &b);

    // Самозванец знает адрес и хочет назваться B. Ключ подписи у него свой —
    // а именно он и проверяется (§2.1).
    let impostor = Device2::new("Самозванец", "мастер-пароль-4");
    let identity = impostor.with_core(|core| core.net_identity().unwrap());

    let mut session = dial(a.addr(), &identity, Mode::Trusted).expect("рукопожатие");
    assert!(session.channel.request(&Msg::Ping).is_err());
    assert_eq!(a.peers().len(), 1, "в доверенных по-прежнему один сосед");
}

#[test]
fn the_channel_is_actually_encrypted() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", "мастер-пароль-2");
    trust_each_other(&a, &b);

    // Играем за постороннего с анализатором трафика: читаем то, что A шлёт в
    // ответ на рукопожатие, и ищем в байтах хоть что-нибудь узнаваемое.
    let identity = b.with_core(|core| core.net_identity().unwrap());
    let mut session = dial(a.addr(), &identity, Mode::Trusted).expect("рукопожатие");

    assert_eq!(session.channel.request(&Msg::Ping).unwrap(), Msg::Pong);

    // Приветствие открытое — до обмена ключами шифровать нечем, — но имени
    // машины в нём нет: `Hello` несёт только версию, режим, ключи и случайность.
    let hello = syncra_core::net::wire::Hello {
        role: syncra_core::net::wire::Role::Initiator,
        mode: Mode::Trusted,
        signing_public: [0u8; 32],
        ecdh_public: [0u8; 32],
        nonce: [0u8; 32],
    }
    .encode();
    assert!(!String::from_utf8_lossy(&hello).contains("Ноутбук"));
    assert!(!String::from_utf8_lossy(&hello).contains("Телефон"));

    // А содержательные кадры вообще неотличимы от шума: тот же `Ping`, дважды
    // отправленный, даёт на проводе разные байты (счётчик nonce двигается).
    let plain = Msg::Ping.encode().unwrap();
    assert_eq!(plain.len(), 1, "открытый Ping — один байт тега");
    assert_eq!(session.channel.request(&Msg::Ping).unwrap(), Msg::Pong);
}

fn dial(
    addr: SocketAddr,
    identity: &syncra_core::net::NetIdentity,
    mode: Mode,
) -> Result<handshake::Established, syncra_core::CoreError> {
    let stream = std::net::TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    handshake::initiate(
        stream,
        &identity.keypair,
        mode,
        handshake::ephemeral_secret()?,
    )
}

// ---------------------------------------------------------------------------
// Ручной код: то, ради чего S3 понадобился сопряжению
// ---------------------------------------------------------------------------

#[test]
fn a_hand_typed_code_finds_the_device_nearby() {
    let shows = Device2::new("Ноутбук", MASTER_PASSWORD);
    let reads = Device2::new("Телефон", "мастер-пароль-2");
    reads.node.seed_peer(shows.addr());

    let offer = shows
        .with_core(|core| core.get_pairing_payload())
        .expect("код сопряжения");

    // Шесть символов с чужого экрана — с пробелами и в нижнем регистре, как их
    // и набирает человек.
    let typed = format!(
        " {} · {} ",
        offer.manual_code[..3].to_lowercase(),
        offer.manual_code[3..].to_lowercase()
    );
    let handshake = reads
        .node
        .submit_paired_key(&typed)
        .expect("устройство найдено рядом");

    assert_eq!(handshake.peer_name, "Ноутбук");
    assert_eq!(handshake.peer_kind, DeviceKind::Desktop);
    assert_eq!(handshake.fingerprint_words.len(), 4);

    // Ровно те же слова получаются из пейлоада, прочитанного камерой: путь через
    // сеть приводит к тому же отпечатку, что и путь через QR, — иначе сверять на
    // двух экранах было бы нечего (§2.2).
    let scanned = shows.with_core(|core| core.shown_pairing_payload().unwrap().to_owned());
    let by_camera = Device2::new("Планшет", "мастер-пароль-3")
        .with_core(|core| core.submit_paired_key(&scanned))
        .unwrap();
    assert_ne!(
        by_camera.fingerprint_words, handshake.fingerprint_words,
        "у другого устройства ключи другие, значит и слова другие"
    );

    // Устройство в доверенные пока не записано: слова возвращаются ДО этого.
    assert!(reads.peers().is_empty());
}

#[test]
fn a_wrong_code_finds_nothing() {
    let shows = Device2::new("Ноутбук", MASTER_PASSWORD);
    let reads = Device2::new("Телефон", "мастер-пароль-2");
    reads.node.seed_peer(shows.addr());

    shows.with_core(|core| core.get_pairing_payload()).unwrap();

    // Код правильной формы, но не тот. Ответ тот же, что был до появления сети:
    // рядом с этим кодом никого не нашлось.
    assert_code(
        reads.node.submit_paired_key("4TQ9MB"),
        CoreErrorCode::NotFound,
    );
    // ...а мусор по-прежнему не доходит даже до сети.
    assert_code(
        reads.node.submit_paired_key("привет"),
        CoreErrorCode::Validation,
    );
}

#[test]
fn guessing_the_code_burns_the_offer() {
    let shows = Device2::new("Ноутбук", MASTER_PASSWORD);
    let reads = Device2::new("Телефон", "мастер-пароль-2");
    reads.node.seed_peer(shows.addr());

    let offer = shows.with_core(|core| core.get_pairing_payload()).unwrap();

    // Шесть символов — это тридцать бит, и в локальной сети они перебираются за
    // секунды. Поэтому считается не длина кода, а число промахов.
    for guess in ["222222", "333333", "444444", "555555", "666666"] {
        assert_ne!(
            guess, offer.manual_code,
            "догадка не должна случайно совпасть"
        );
        assert_code(reads.node.submit_paired_key(guess), CoreErrorCode::NotFound);
    }

    // Попытки кончились — код сгорел, и НАСТОЯЩИЙ код больше не работает.
    assert_code(
        reads.node.submit_paired_key(&offer.manual_code),
        CoreErrorCode::NotFound,
    );
}

#[test]
fn the_side_that_showed_the_code_learns_about_it() {
    let shows = Device2::new("Ноутбук", MASTER_PASSWORD);
    let reads = Device2::new("Телефон", "мастер-пароль-2");
    reads.node.seed_peer(shows.addr());

    let offer = shows.with_core(|core| core.get_pairing_payload()).unwrap();
    let handshake = reads
        .node
        .submit_paired_key(&offer.manual_code)
        .expect("устройство найдено");
    let result = reads
        .node
        .confirm_pairing(&handshake.session_id)
        .expect("подтверждение");

    assert_eq!(result.device.name, "Ноутбук");
    assert_eq!(result.records_transferred, 0, "переносить нечем до S4");

    // А вот и то, ради чего всё это: показавшая сторона ничего не вызывала.
    let paired = shows.wait_event(|event| matches!(event, CoreEvent::DevicePaired(_)));
    let CoreEvent::DevicePaired(result) = paired else {
        unreachable!()
    };
    assert_eq!(result.device.name, "Телефон");
    assert_eq!(result.records_transferred, 0);

    // Обе стороны теперь знают друг друга, и слова у них одинаковые.
    let here = shows.peers();
    let there = reads.peers();
    assert_eq!(here.len(), 1);
    assert_eq!(there.len(), 1);
    assert_eq!(here[0].fingerprint_words, there[0].fingerprint_words);
    assert_eq!(here[0].fingerprint_words, handshake.fingerprint_words);
}

#[test]
fn a_used_code_does_not_pair_a_second_device() {
    let shows = Device2::new("Ноутбук", MASTER_PASSWORD);
    let first = Device2::new("Телефон", "мастер-пароль-2");
    let second = Device2::new("Планшет", "мастер-пароль-3");
    first.node.seed_peer(shows.addr());
    second.node.seed_peer(shows.addr());

    let offer = shows.with_core(|core| core.get_pairing_payload()).unwrap();
    let handshake = first.node.submit_paired_key(&offer.manual_code).unwrap();
    first.node.confirm_pairing(&handshake.session_id).unwrap();
    shows.wait_event(|event| matches!(event, CoreEvent::DevicePaired(_)));

    // Код сделал своё дело и сгорел: одноразовый ключ на то и одноразовый.
    assert_code(
        second.node.submit_paired_key(&offer.manual_code),
        CoreErrorCode::NotFound,
    );
    assert_eq!(shows.peers().len(), 1);
}

// ---------------------------------------------------------------------------
// События и статус
// ---------------------------------------------------------------------------

#[test]
fn a_found_peer_moves_last_seen_at() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", "мастер-пароль-2");

    let before = {
        trust_each_other(&a, &b);
        a.peers()[0].last_seen_at.clone()
    };
    a.wait_event(|event| matches!(event, CoreEvent::PeerFound(_)));

    let after = a.peers()[0].last_seen_at.clone();
    assert!(after.is_some());
    assert!(
        after > before || after != before,
        "{after:?} против {before:?}"
    );
}

#[test]
fn a_revoked_device_is_never_counted_as_a_peer() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let b = Device2::new("Телефон", "мастер-пароль-2");
    trust_each_other(&a, &b);
    a.wait_event(|event| matches!(event, CoreEvent::PeerFound(_)));

    let peer_id = a.peers()[0].device_id.clone();
    a.with_core(|core| core.revoke_device(&peer_id)).unwrap();

    // Отозванное устройство в сети видно — оно никуда не делось, — но пиром
    // быть перестало: ни в счётчике, ни в событиях (§2.3).
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if a.node.status().peers_online == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(a.node.status().peers_online, 0);
}

#[test]
fn the_status_carries_the_whole_truth() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);

    // Сразу после отпирания — «ищем»; это состояние временное и должно пройти
    // само, иначе индикатор пульсировал бы вечно там, где рядом просто никого.
    let start = a.node.status();
    assert_eq!(start.phase, SyncPhase::Searching);
    assert_eq!(start.peers_online, 0);
    assert!(start.peer_name.is_none());
    assert!(start.pending_records.is_empty());
    assert!(start.last_sync_at.is_none());
    assert!(start.message.is_none());

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && a.node.status().phase == SyncPhase::Searching {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(a.node.status().phase, SyncPhase::Idle);

    // «Синхронизировать сейчас» перезапускает поиск, не дожидаясь круга.
    assert_eq!(a.node.sync_now().phase, SyncPhase::Searching);
}

#[test]
fn a_locked_core_is_not_on_the_network() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);
    let addr = a.addr();
    assert!(a.node.is_running());

    a.with_core(|core| core.lock());
    a.node.tick();

    assert!(!a.node.is_running());
    assert!(a.node.local_addr().is_none());
    assert_eq!(a.node.status().phase, SyncPhase::Idle);
    assert_eq!(a.node.status().peers_online, 0);

    // Слушатель закрыт: отвечать на запертом хранилище нечем — ключей нет.
    let stranger = Device2::new("Чужой", "мастер-пароль-3");
    let identity = stranger.with_core(|core| core.net_identity().unwrap());
    assert!(dial(addr, &identity, Mode::Trusted).is_err());
}

#[test]
fn unlocking_puts_the_device_back_on_the_network() {
    let a = Device2::new("Ноутбук", MASTER_PASSWORD);

    a.with_core(|core| core.lock());
    a.node.tick();
    assert!(!a.node.is_running());

    a.with_core(|core| core.unlock(MASTER_PASSWORD)).unwrap();
    a.node.tick();

    assert!(a.node.is_running());
    assert!(a.node.local_addr().is_some());
}

// ---------------------------------------------------------------------------
// Настоящий mDNS — руками
// ---------------------------------------------------------------------------

/// Два узла с включённым обнаружением находят друг друга в живой сети.
///
/// `#[ignore]` намеренно: мультикаст зависит от брандмауэра, от виртуальных
/// адаптеров и от того, что ещё крутится в сети у запускающего. Такому тесту
/// место в руках человека, а не в `cargo test`:
///
/// ```sh
/// cargo test -p syncra-core --test net -- --ignored --nocapture
/// ```
#[test]
#[ignore]
fn mdns_finds_a_real_neighbour() {
    let settings = NodeSettings {
        discovery: true,
        probe_interval: Duration::from_millis(500),
        ..settings()
    };
    let a = Device2::with_settings("Ноутбук", MASTER_PASSWORD, settings.clone());
    let b = Device2::with_settings("Телефон", "мастер-пароль-2", settings);

    // Сопрягаем напрямую, без сети: проверяется именно ПОИСК.
    pair(&a, &b);

    let found = a.wait_event(|event| matches!(event, CoreEvent::PeerFound(_)));
    let CoreEvent::PeerFound(peer) = found else {
        unreachable!()
    };
    println!("нашли: {} ({})", peer.name, peer.device_id);
    assert_eq!(peer.name, "Телефон");
}
