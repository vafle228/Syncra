//! Общая обвязка приёмочных тестов ядра.
//!
//! Модуль подключается в каждый тестовый бинарник целиком, и каждому нужна своя
//! часть — отсюда `dead_code`.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use syncra_core::net::CoreEvent;
use syncra_core::{
    Core, CoreError, CoreErrorCode, Device, HostDevice, Node, NodeSettings, RecordDraft,
};

pub const MASTER_PASSWORD: &str = "мастер-пароль-1";
/// Имя, которым в тестах подписывается это устройство. В приложении его даёт
/// оболочка (hostname); ядру оно приходит готовым, как и путь к БД.
pub const HOST_NAME: &str = "Стенд";

pub fn host() -> HostDevice {
    HostDevice::desktop(HOST_NAME)
}

/// Свежесозданное открытое хранилище: ровно то состояние, в котором пользователь
/// оказывается сразу после онбординга.
pub fn unlocked() -> Core {
    let mut core = Core::in_memory(host()).expect("хранилище в памяти");
    core.init_vault(MASTER_PASSWORD).expect("init_vault");
    core
}

pub fn draft(service_name: &str, login: &str, password: &str) -> RecordDraft {
    RecordDraft {
        vault_id: None,
        service_name: service_name.to_owned(),
        urls: vec![format!("{}.com", service_name.to_lowercase())],
        login: login.to_owned(),
        account_label: None,
        password: password.to_owned(),
        notes: None,
        totp_secret: None,
    }
}

/// Проверяет код ошибки и заодно печатает сообщение при расхождении — иначе
/// упавший тест сообщает только «не тот вариант enum».
#[track_caller]
pub fn assert_code<T: std::fmt::Debug>(result: Result<T, CoreError>, expected: CoreErrorCode) {
    match result {
        Ok(value) => panic!("ждали {expected:?}, получили Ok({value:?})"),
        Err(error) => assert_eq!(error.code, expected, "ждали {expected:?}, получили {error}"),
    }
}

// ---------------------------------------------------------------------------
// Сетевой стенд: два узла на loopback
// ---------------------------------------------------------------------------
//
// «Второе устройство» здесь — второй узел в том же процессе, на своём хранилище
// и со своим мастер-паролем; разговаривают они через настоящий TCP на loopback.
// Это ровно то, что происходит в жизни, минус километр провода.
//
// Живёт стенд здесь, а не в одном файле тестов, потому что нужен двоим: `net.rs`
// проверяет по нему рукопожатие и сопряжение, `sync.rs` — обмен записями.

/// Сроки, ужатые до тестовых: ждать по пятнадцать секунд здесь нечего.
pub fn settings() -> NodeSettings {
    NodeSettings {
        discovery: false,
        port: 0,
        io_timeout: Duration::from_secs(3),
        connect_timeout: Duration::from_millis(800),
        probe_interval: Duration::from_millis(150),
        peer_ttl: Duration::from_millis(600),
        stranger_backoff: Duration::from_millis(200),
        search_window: Duration::from_millis(300),
        // Круги идут часто: тесту нечего ждать по минуте, а проверяется в них
        // не расписание, а сам обмен.
        sync_interval: Duration::from_millis(200),
    }
}

/// Устройство целиком: ядро, узел и очередь его событий.
pub struct Device2 {
    pub core: Arc<Mutex<Core>>,
    pub node: Node,
    pub events: Receiver<CoreEvent>,
}

impl Device2 {
    pub fn new(name: &str, master_password: &str) -> Self {
        Self::with_settings(name, master_password, settings())
    }

    pub fn with_settings(name: &str, master_password: &str, settings: NodeSettings) -> Self {
        let mut core = Core::in_memory(HostDevice::desktop(name)).expect("хранилище в памяти");
        core.init_vault(master_password).expect("init_vault");

        let core = Arc::new(Mutex::new(core));
        let (node, events) = Node::new(Arc::clone(&core), settings);
        node.tick();
        Self { core, node, events }
    }

    pub fn addr(&self) -> SocketAddr {
        self.node.local_addr().expect("узел слушает порт")
    }

    pub fn with_core<T>(&self, action: impl FnOnce(&mut Core) -> T) -> T {
        action(&mut self.core.lock().unwrap())
    }

    pub fn peers(&self) -> Vec<Device> {
        self.with_core(|core| core.list_devices().unwrap())
            .into_iter()
            .filter(|device| !device.is_this_device)
            .collect()
    }

    /// Дождаться события, подходящего под условие. Дольше секунды здесь ничего
    /// не происходит — а если происходит, тест обязан упасть, а не висеть.
    pub fn wait_event(&self, matches: impl Fn(&CoreEvent) -> bool) -> CoreEvent {
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
pub fn pair(shows: &Device2, reads: &Device2) {
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
pub fn trust_each_other(a: &Device2, b: &Device2) {
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
