//! Сеть: обнаружение, транспорт и всё, что вокруг них (§5.1, §5.6, §8.3).
//!
//! # Закон этого модуля
//!
//! **`Mutex<Core>` не удерживается через сетевое ожидание.** Ядро синхронное и
//! живёт под мьютексом; любой таймаут, взятый под этим замком, замораживает всё
//! приложение — включая кнопку «Запереть». Поэтому здесь есть ровно один способ
//! дотронуться до ядра — [`Context::with_core`], — и внутри его замыкания не
//! бывает ни одного сетевого вызова. Долгое ожидание всегда снаружи замка.
//!
//! Потоки узла держат `Weak<Mutex<Core>>`, а не `Arc`: ядром владеет оболочка,
//! и если она его уронила, потоки это видят и уходят, а не продлевают ему жизнь.
//!
//! # Что здесь происходит
//!
//! ```text
//! слушатель  ── accept ──► рукопожатие ──► доверенный: ping/pong, peer_found
//!                                      └─► сопряжение: показать пейлоад,
//!                                                      принять «я тебя записал»
//! обходчик   ── mDNS ──► адреса ──► проба доверенным рукопожатием ──► peer_found
//! ```
//!
//! Синхронизации здесь нет: манифест и дифф приезжают в S4 — в доверенный режим,
//! рядом с `Ping`.

pub mod channel;
pub mod discovery;
pub mod handshake;
pub mod wire;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use crate::crypto::DeviceKeypair;
use crate::error::{CoreError, CoreResult};
use crate::model::{
    now_iso, DeviceKind, HostDevice, PairingHandshake, PairingResult, PeerFound, SyncPhase,
    SyncStatus,
};
use crate::pairing;
use crate::session::{Core, PairingAnnouncement};

use discovery::{Discovery, Sighting};
use wire::{Mode, Msg, HALF_PUBLIC_LEN};

// ---------------------------------------------------------------------------
// Общие типы
// ---------------------------------------------------------------------------

/// Чем устройство представляется в сети. Собирает [`Core::net_identity`].
pub struct NetIdentity {
    pub device_id: String,
    pub host: HostDevice,
    pub keypair: DeviceKeypair,
}

/// Устройство, представившееся по сети при сопряжении.
pub struct RemotePeer<'a> {
    pub device_id: &'a str,
    pub name: &'a str,
    pub kind: DeviceKind,
    /// Половина обмена ключами. Половину подписи брать с его слов нельзя — она
    /// приходит из рукопожатия, где доказана подписью.
    pub agreement_public: &'a [u8; HALF_PUBLIC_LEN],
}

/// То, о чём ядро рассказывает наружу само, без вопроса от UI.
///
/// Ядро про Tauri не знает: события уходят в канал, а оболочка сливает его и
/// переизлучает уже своим способом.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEvent {
    /// Доверенное устройство на связи (`peer_found`, `contract.ts:1306`).
    PeerFound(PeerFound),
    /// Нас сопрягли с той стороны (`device_paired`, `contract.ts:1318`).
    DevicePaired(Box<PairingResult>),
    /// Полное состояние синхронизации (`sync_status`, `contract.ts:1297`).
    SyncStatus(Box<SyncStatus>),
}

// ---------------------------------------------------------------------------
// Настройки
// ---------------------------------------------------------------------------

/// Сроки и пороги сетевого слоя.
///
/// Все они приходят снаружи, а не берутся из воздуха внутри: так их задают
/// тесты, не проспав в них по минуте (тот же приём, что у `autolock_due` и
/// `pairing::is_expired`).
#[derive(Debug, Clone)]
pub struct NodeSettings {
    /// Анонсировать себя и искать соседей по mDNS.
    ///
    /// Выключается тестами: настоящий мультикаст в `cargo test` зависит от
    /// брандмауэра и от того, что ещё крутится в сети, — то есть проверял бы не
    /// ядро. Транспорт при этом проверяется целиком: адреса приходят готовыми.
    pub discovery: bool,
    /// Порт слушателя. Ноль — любой свободный.
    pub port: u16,
    /// Потолок ожидания на сокете.
    pub io_timeout: Duration,
    /// Потолок ожидания соединения.
    pub connect_timeout: Duration,
    /// Как часто обходчик звонит известным адресам.
    pub probe_interval: Duration,
    /// Сколько устройство считается «на связи» после последнего успеха.
    pub peer_ttl: Duration,
    /// Сколько не звонить по адресу, который оказался чужим.
    pub stranger_backoff: Duration,
    /// Сколько показывать «ищем устройства», прежде чем успокоиться.
    pub search_window: Duration,
}

impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            discovery: true,
            port: 0,
            io_timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_millis(1500),
            // Пятнадцать секунд — компромисс: «телефон появился в сети» человек
            // готов подождать столько, а будить процесс чаще незачем.
            probe_interval: Duration::from_secs(15),
            // Три пропущенные пробы: одна неудача бывает от чего угодно.
            peer_ttl: Duration::from_secs(45),
            // Соседский принтер не должен получать звонок каждые пятнадцать
            // секунд до конца рабочего дня.
            stranger_backoff: Duration::from_secs(300),
            // Дольше пульсирующий индикатор врёт: если рядом никого, это надо
            // сказать спокойно, а не искать вечно.
            search_window: Duration::from_secs(10),
        }
    }
}

/// Как часто слушатель просыпается посмотреть, не пора ли уходить.
const ACCEPT_POLL: Duration = Duration::from_millis(200);

/// Шаг обходчика. Не то же, что `probe_interval`: обходчик просыпается чаще,
/// чтобы вовремя заметить остановку и внеочередную просьбу поискать.
const WALK_STEP: Duration = Duration::from_millis(250);

/// Сколько раз доставка «я тебя записал» пробует достучаться. Дальше сроку
/// оффера всё равно конец.
const ANNOUNCE_ATTEMPTS: u32 = 6;

/// Пауза между попытками доставки.
const ANNOUNCE_PAUSE: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// Общее состояние
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Peers {
    /// Что видно в сети. Кто это — ещё неизвестно.
    addrs: Vec<SocketAddr>,
    /// Адрес → до какого момента в него не звонить (там не Syncra или не наш).
    strangers: HashMap<SocketAddr, Instant>,
    /// `device_id` → когда доверенное устройство последний раз отозвалось.
    online: HashMap<String, Instant>,
}

struct Shared {
    /// Номер поколения. Растёт на каждом старте; потоки прошлых поколений это
    /// видят и уходят. Проще и надёжнее, чем будить каждый поток по отдельности.
    generation: AtomicU64,
    running: AtomicBool,
    peers: Mutex<Peers>,
    /// Докуда показывать «ищем устройства».
    searching_until: Mutex<Option<Instant>>,
    /// Внеочередная просьба поискать (`sync_now`).
    probe_requested: AtomicBool,
    /// Почему сеть не поднялась. Непусто только в фазе `error`.
    failure: Mutex<Option<String>>,
    /// Последний разосланный статус — чтобы не слать одно и то же дважды.
    last_status: Mutex<Option<SyncStatus>>,
    events: Sender<CoreEvent>,
    port: AtomicU64,
}

impl Shared {
    /// Забыть, кто был признан чужим.
    ///
    /// Зовётся, когда таблица доверия изменилась сопряжением: вердикт «это не
    /// наш» вынесен по старому составу `devices` и после нового сопряжения
    /// просто неверен. Без этого только что сопряжённое (или возвращённое из
    /// отзыва) устройство ждало бы своей пробы до конца отката.
    fn forget_strangers(&self) {
        if let Ok(mut peers) = self.peers.lock() {
            peers.strangers.clear();
        }
    }

    fn announce(&self, event: CoreEvent) {
        // Событие — не отчёт об успехе: некому доставить, значит некому. Ронять
        // из-за этого сетевой поток нельзя (та же логика, что в `announce`
        // оболочки).
        let _ = self.events.send(event);
    }
}

// ---------------------------------------------------------------------------
// Контекст рабочих потоков
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Context {
    core: Weak<Mutex<Core>>,
    shared: Arc<Shared>,
    settings: NodeSettings,
    generation: u64,
}

impl Context {
    /// ЕДИНСТВЕННЫЙ способ дотронуться до ядра из сетевого потока.
    ///
    /// Замок берётся на время замыкания и отпускается сразу. Ни одного сетевого
    /// вызова внутри этого замыкания быть не должно — весь модуль написан так,
    /// чтобы такое место было видно глазом.
    ///
    /// `None` означает «ядра больше нет или оно в несогласованном состоянии» —
    /// и то и другое для сетевого потока значит одно: тихо уйти.
    fn with_core<T>(&self, action: impl FnOnce(&mut Core) -> T) -> Option<T> {
        let core = self.core.upgrade()?;
        let mut guard = core.lock().ok()?;
        Some(action(&mut guard))
    }

    /// Не пора ли этому потоку уйти.
    fn stale(&self) -> bool {
        self.shared.generation.load(Ordering::SeqCst) != self.generation
    }

    fn identity(&self) -> Option<NetIdentity> {
        self.with_core(|core| core.net_identity().ok())?
    }
}

// ---------------------------------------------------------------------------
// Узел
// ---------------------------------------------------------------------------

/// Сетевая сторона ядра.
///
/// Держит `Arc<Mutex<Core>>` и всё, что вокруг него крутится в своих потоках.
/// Команды, которым нужна сеть, идут через узел, а не прямо в ядро: только так
/// ожидание оказывается снаружи замка.
pub struct Node {
    core: Arc<Mutex<Core>>,
    shared: Arc<Shared>,
    settings: NodeSettings,
}

impl Node {
    /// Завести узел. Сеть при этом ещё не поднимается — это делает [`Node::tick`]
    /// на отпертом хранилище.
    pub fn new(core: Arc<Mutex<Core>>, settings: NodeSettings) -> (Self, Receiver<CoreEvent>) {
        let (events, inbox) = mpsc::channel();
        let shared = Arc::new(Shared {
            generation: AtomicU64::new(0),
            running: AtomicBool::new(false),
            peers: Mutex::new(Peers::default()),
            searching_until: Mutex::new(None),
            probe_requested: AtomicBool::new(false),
            failure: Mutex::new(None),
            last_status: Mutex::new(None),
            events,
            port: AtomicU64::new(0),
        });
        (
            Self {
                core,
                shared,
                settings,
            },
            inbox,
        )
    }

    /// Шаг сторожа: поднять сеть на отпертом хранилище, опустить на запертом.
    ///
    /// Зовётся оболочкой раз в секунду, рядом с проверкой автоблокировки.
    ///
    /// **Почему тик, а не вызов из `Core::unlock`.** `unlock` работает ПОД
    /// мьютексом, а старту сети мьютекс нужен, чтобы прочитать ключи, — это был
    /// бы дедлок в первой же строчке. Зернистость в секунду здесь та же, что у
    /// автоблокировки, и приемлема по той же причине.
    pub fn tick(&self) {
        let Ok(core) = self.core.lock() else {
            return;
        };
        let unlocked = core.is_unlocked();
        drop(core);

        match (unlocked, self.shared.running.load(Ordering::SeqCst)) {
            (true, false) => self.start(),
            (false, true) => self.stop(),
            _ => {}
        }
    }

    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst)
    }

    /// Адрес слушателя. Нужен тестам и тому, кто сводит два экземпляра руками.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        let port = self.shared.port.load(Ordering::SeqCst) as u16;
        (port != 0).then(|| SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
    }

    /// Подсказать узлу адрес соседа, минуя обнаружение.
    ///
    /// Существует для тестов и для случая, когда mDNS в сети запрещён. Доверия
    /// это не даёт никакого: адрес только говорит, куда звонить, а пускать или
    /// нет решает рукопожатие (§2.1).
    pub fn seed_peer(&self, addr: SocketAddr) {
        if let Ok(mut peers) = self.shared.peers.lock() {
            if !peers.addrs.contains(&addr) {
                peers.addrs.push(addr);
            }
            peers.strangers.remove(&addr);
        }
    }

    /// Текущее состояние синхронизации целиком.
    pub fn status(&self) -> SyncStatus {
        compute_status(&self.shared, Instant::now())
    }

    /// «Синхронизировать сейчас» (F10).
    ///
    /// **Граница шага:** обмена ещё нет — есть поиск. Кнопка честно перезапускает
    /// обход соседей и не ждёт следующего круга; манифест и дифф приедут в S4.
    pub fn sync_now(&self) -> SyncStatus {
        if let Ok(mut until) = self.shared.searching_until.lock() {
            *until = Some(Instant::now() + self.settings.search_window);
        }
        self.shared.probe_requested.store(true, Ordering::SeqCst);
        publish_status(&self.shared, Instant::now())
    }

    // -----------------------------------------------------------------------
    // Команды, которым нужна сеть
    // -----------------------------------------------------------------------

    /// Отдать ядру прочитанное со второго устройства (F8).
    ///
    /// Разбор — чистая функция, замок для неё не нужен; поиск по шести символам
    /// идёт СНАРУЖИ замка. Это и есть причина, по которой команда ходит через
    /// узел, а не прямо в ядро.
    pub fn submit_paired_key(&self, input: &str) -> CoreResult<PairingHandshake> {
        let payload = match pairing::parse(input)? {
            pairing::Parsed::Payload { .. } => input.to_owned(),
            pairing::Parsed::Code(code) => {
                // Про замок человек должен узнать сразу, а не после того, как мы
                // отходим все таймауты по соседям впустую.
                self.lock_core(|core| core.guard_unlocked())??;
                self.fetch_pairing_payload(&code)?
            }
        };
        self.lock_core(|core| core.submit_paired_key(&payload))?
    }

    /// Человек сверил слова и подтвердил (F8).
    ///
    /// Сверх того, что делает ядро, узел берёт на себя доставку: показавшая код
    /// сторона ничего не вызывала и узнает об успехе только по сети.
    pub fn confirm_pairing(&self, session_id: &str) -> CoreResult<PairingResult> {
        let result = self.lock_core(|core| core.confirm_pairing(session_id))??;
        self.shared.forget_strangers();
        let announcement = self.lock_core(|core| core.take_pairing_announcement())?;

        if let Some(announcement) = announcement {
            let context = self.context();
            // В фоне и уже без замка: доставка ходит по сети и может не дойти,
            // а сопряжение здесь уже состоялось и от неё не зависит.
            std::thread::spawn(move || announce_pairing(&context, &announcement));
        }
        Ok(result)
    }

    // -----------------------------------------------------------------------
    // Подъём и остановка
    // -----------------------------------------------------------------------

    fn start(&self) {
        let generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let listener = match TcpListener::bind((Ipv4Addr::UNSPECIFIED, self.settings.port))
            .and_then(|listener| listener.set_nonblocking(true).map(|()| listener))
        {
            Ok(listener) => listener,
            Err(_) => return self.fail("Не удалось открыть сетевой порт для синхронизации."),
        };
        let port = match listener.local_addr() {
            Ok(addr) => addr.port(),
            Err(_) => return self.fail("Не удалось открыть сетевой порт для синхронизации."),
        };

        let discovery = if self.settings.discovery {
            match Discovery::start(port) {
                Ok(discovery) => discovery,
                Err(error) => return self.fail(error.message),
            }
        } else {
            Discovery::Off
        };

        if let Ok(mut failure) = self.shared.failure.lock() {
            *failure = None;
        }
        if let Ok(mut until) = self.shared.searching_until.lock() {
            *until = Some(Instant::now() + self.settings.search_window);
        }
        self.shared.port.store(u64::from(port), Ordering::SeqCst);
        self.shared.running.store(true, Ordering::SeqCst);

        let context = Context {
            core: Arc::downgrade(&self.core),
            shared: Arc::clone(&self.shared),
            settings: self.settings.clone(),
            generation,
        };
        let accepting = context.clone();
        std::thread::spawn(move || accept_loop(&accepting, listener));
        std::thread::spawn(move || walk_loop(&context, discovery));

        publish_status(&self.shared, Instant::now());
    }

    fn stop(&self) {
        // Смена поколения — весь сигнал остановки: потоки заметят её на
        // ближайшем шаге и уйдут, закрыв за собой сокеты.
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        self.shared.running.store(false, Ordering::SeqCst);
        self.shared.port.store(0, Ordering::SeqCst);

        if let Ok(mut peers) = self.shared.peers.lock() {
            *peers = Peers::default();
        }
        if let Ok(mut until) = self.shared.searching_until.lock() {
            *until = None;
        }
        publish_status(&self.shared, Instant::now());
    }

    fn fail(&self, message: impl Into<String>) {
        if let Ok(mut failure) = self.shared.failure.lock() {
            *failure = Some(message.into());
        }
        self.shared.running.store(false, Ordering::SeqCst);
        self.shared.port.store(0, Ordering::SeqCst);
        publish_status(&self.shared, Instant::now());
    }

    fn context(&self) -> Context {
        Context {
            core: Arc::downgrade(&self.core),
            shared: Arc::clone(&self.shared),
            settings: self.settings.clone(),
            generation: self.shared.generation.load(Ordering::SeqCst),
        }
    }

    fn lock_core<T>(&self, action: impl FnOnce(&mut Core) -> T) -> CoreResult<T> {
        let mut core = self
            .core
            .lock()
            .map_err(|_| CoreError::internal("Ядро в несогласованном состоянии."))?;
        Ok(action(&mut core))
    }

    /// Найти рядом устройство, показывающее этот код (F8, §2.2).
    ///
    /// Идёт СНАРУЖИ замка ядра — в этом весь смысл метода.
    fn fetch_pairing_payload(&self, code: &str) -> CoreResult<String> {
        let context = self.context();
        // Список «чужих» здесь НЕ учитывается, и это не упущение: чужой — это
        // ровно тот, с кем мы и собираемся сопрягаться. Откат придуман для
        // пробы доверенных, чтобы не звонить соседскому принтеру каждые
        // пятнадцать секунд; сопряжение — действие человека и разовое.
        let addrs = context.shared.peers.lock().map(|peers| peers.addrs.clone());

        for addr in addrs.unwrap_or_default() {
            if let Some(payload) = ask_for_payload(&context, addr, code) {
                return Ok(payload);
            }
        }

        // Формулировка та же, что была до появления сети: она и писалась так,
        // чтобы остаться правдой. Изменилось только то, кто её даёт.
        Err(CoreError::not_found(
            "Устройство с этим кодом не найдено рядом. Отсканируйте QR-код \
             со второго устройства или откройте файл с кодом.",
        ))
    }
}

// ---------------------------------------------------------------------------
// Статус
// ---------------------------------------------------------------------------

fn compute_status(shared: &Shared, now: Instant) -> SyncStatus {
    if let Some(message) = shared
        .failure
        .lock()
        .ok()
        .and_then(|failure| failure.clone())
    {
        return SyncStatus {
            phase: SyncPhase::Error,
            message: Some(message),
            ..SyncStatus::idle()
        };
    }
    if !shared.running.load(Ordering::SeqCst) {
        return SyncStatus::idle();
    }

    let online = shared
        .peers
        .lock()
        .map(|peers| peers.online.len() as u32)
        .unwrap_or(0);
    let searching = online == 0
        && shared
            .searching_until
            .lock()
            .ok()
            .and_then(|until| *until)
            .is_some_and(|until| now < until);

    SyncStatus {
        // `idle` с найденными соседями и пустым списком ожидающих — это и есть
        // «всё сошлось»: остальные пять видов индикатора UI выводит сам.
        phase: if searching {
            SyncPhase::Searching
        } else {
            SyncPhase::Idle
        },
        peers_online: online,
        // ГРАНИЦА ШАГА: обмена ещё нет, и заполнять эти поля нечем (S4).
        peer_name: None,
        pending_records: Vec::new(),
        last_sync_at: None,
        message: None,
    }
}

/// Разослать статус, если он изменился.
///
/// Уходит ПОЛНЫЙ статус, а не дельта: подписка живёт не всё время работы окна, и
/// собранное по кусочкам состояние разъехалось бы с ядром на первом же
/// пропущенном событии (`contract.ts:1290`).
fn publish_status(shared: &Shared, now: Instant) -> SyncStatus {
    let status = compute_status(shared, now);

    let changed = match shared.last_status.lock() {
        Ok(mut last) => {
            let changed = last.as_ref() != Some(&status);
            if changed {
                *last = Some(status.clone());
            }
            changed
        }
        Err(_) => false,
    };
    if changed {
        shared.announce(CoreEvent::SyncStatus(Box::new(status.clone())));
    }
    status
}

/// Устройство отозвалось: подвинуть `last_seen_at`, сказать про него, если оно
/// только что появилось.
fn mark_online(context: &Context, device_id: &str) {
    let fresh = match context.shared.peers.lock() {
        Ok(mut peers) => peers
            .online
            .insert(device_id.to_owned(), Instant::now())
            .is_none(),
        Err(_) => return,
    };

    let device = context.with_core(|core| core.mark_peer_seen(device_id).ok().flatten());
    let Some(Some(device)) = device else {
        return;
    };

    if fresh {
        context.shared.announce(CoreEvent::PeerFound(PeerFound {
            device_id: device.device_id,
            name: device.name,
            kind: device.kind,
            found_at: device.last_seen_at.unwrap_or_else(now_iso),
        }));
    }
    publish_status(&context.shared, Instant::now());
}

/// Убрать тех, кто давно не отзывался.
///
/// Отдельного события об уходе в контракте нет намеренно: уход виден по
/// упавшему `peers_online` в общем статусе.
fn expire_online(context: &Context, now: Instant) {
    let dropped = match context.shared.peers.lock() {
        Ok(mut peers) => {
            let before = peers.online.len();
            peers
                .online
                .retain(|_, seen| now.duration_since(*seen) < context.settings.peer_ttl);
            before != peers.online.len()
        }
        Err(_) => false,
    };
    if dropped {
        publish_status(&context.shared, now);
    }
}

// ---------------------------------------------------------------------------
// Слушатель
// ---------------------------------------------------------------------------

fn accept_loop(context: &Context, listener: TcpListener) {
    while !context.stale() {
        match listener.accept() {
            Ok((stream, _)) => {
                let context = context.clone();
                // Каждое соединение — свой поток: разговор с одним соседом не
                // должен мешать принять второго.
                std::thread::spawn(move || {
                    let _ = serve(&context, stream);
                });
            }
            // Слушатель неблокирующий, чтобы замечать остановку: без этого
            // `accept` держал бы поток до следующего звонка, которого может и
            // не быть.
            Err(_) => std::thread::sleep(ACCEPT_POLL),
        }
    }
}

fn serve(context: &Context, stream: TcpStream) -> CoreResult<()> {
    // Принятый сокет наследует неблокирующий режим слушателя — а разговор с ним
    // ведётся обычными чтениями с таймаутом.
    stream.set_nonblocking(false)?;
    let stream = channel::with_timeouts(stream, context.settings.io_timeout)?;

    let identity = context
        .identity()
        .ok_or_else(|| CoreError::internal("Хранилище заперто."))?;

    let secret_for = |mode: Mode| match mode {
        // Показываем код — подписываем канал ключом СЕАНСА из пейлоада: тот, кто
        // читал QR, узнает по нему тот самый экран (§2.2).
        Mode::Pairing => match context
            .with_core(|core| core.offer_session_secret())
            .flatten()
        {
            Some(secret) => Ok(secret),
            None => handshake::ephemeral_secret(),
        },
        Mode::Trusted => handshake::ephemeral_secret(),
    };
    let established = handshake::accept(stream, &identity.keypair, secret_for)?;

    match established.mode {
        Mode::Trusted => serve_trusted(context, established),
        Mode::Pairing => serve_pairing(context, established),
    }
}

fn serve_trusted(context: &Context, mut session: handshake::Established) -> CoreResult<()> {
    // Вот здесь и решается доверие — по ключу, и только по нему. Устройства,
    // которого нет в `devices`, и отозванного здесь не бывает (§2.1, §2.3).
    let Some(Some(device)) =
        context.with_core(|core| core.authorize_peer(&session.peer_signing).ok().flatten())
    else {
        return Err(CoreError::internal(
            "Устройство не входит в число доверенных.",
        ));
    };
    mark_online(context, &device.device_id);

    // ГРАНИЦА ШАГА: доверенный разговор пока состоит из «ты жив?» — «жив».
    // Манифест и дифф встанут сюда же, в S4.
    while !context.stale() {
        match session.channel.recv() {
            Ok(Msg::Ping) => {
                session.channel.send(&Msg::Pong)?;
                // Разговор идёт — значит устройство на связи прямо сейчас.
                // Без этого отметка протухла бы по сроку на стороне, которая
                // только отвечает и сама никому не звонит.
                mark_online(context, &device.device_id);
            }
            // Всё остальное — либо конец разговора, либо непонятный кадр; в обоих
            // случаях вешаем трубку.
            _ => break,
        }
    }
    Ok(())
}

fn serve_pairing(context: &Context, mut session: handshake::Established) -> CoreResult<()> {
    let transcript = session.transcript;

    match session.channel.recv()? {
        Msg::PairingLookup { commitment } => {
            let payload = context
                .with_core(|core| {
                    core.serve_pairing_lookup(&transcript, &commitment)
                        .ok()
                        .flatten()
                })
                .flatten();
            let answer = match payload {
                Some(payload) => Msg::PairingOffer { payload },
                // Нет сеанса, вышел срок, кончились попытки — один ответ на все
                // случаи: подбирающему код незачем знать, теплее или холоднее.
                None => Msg::PairingRefused,
            };
            session.channel.send(&answer)?;
        }
        Msg::PairingComplete {
            commitment,
            device_id,
            name,
            kind,
            agreement_public,
        } => {
            let peer_signing = session.peer_signing;
            let result = context
                .with_core(|core| {
                    core.accept_remote_pairing(
                        &transcript,
                        &commitment,
                        &RemotePeer {
                            device_id: &device_id,
                            name: &name,
                            kind,
                            agreement_public: &agreement_public,
                        },
                        &peer_signing,
                    )
                    .ok()
                    .flatten()
                })
                .flatten();

            match result {
                Some(result) => {
                    session.channel.send(&Msg::PairingAck)?;
                    context.shared.forget_strangers();
                    // Вот ради этого события S3 и нужен сопряжению: здесь код
                    // показывали и никакой команды не вызывали.
                    context
                        .shared
                        .announce(CoreEvent::DevicePaired(Box::new(result)));
                }
                None => session.channel.send(&Msg::PairingRefused)?,
            }
        }
        // В режиме сопряжения больше ничего не разрешено: он и открыт-то ровно
        // для этих двух запросов.
        _ => return Err(wire::malformed()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Обходчик
// ---------------------------------------------------------------------------

fn walk_loop(context: &Context, discovery: Discovery) {
    let mut next_probe = Instant::now();

    while !context.stale() {
        let now = Instant::now();
        absorb(context, discovery.drain());

        let asked = context.shared.probe_requested.swap(false, Ordering::SeqCst);
        if asked || now >= next_probe {
            probe_round(context);
            next_probe = Instant::now() + context.settings.probe_interval;
        }
        expire_online(context, Instant::now());
        publish_status(&context.shared, Instant::now());

        std::thread::sleep(WALK_STEP);
    }
    discovery.shutdown();
}

fn absorb(context: &Context, sightings: Vec<Sighting>) {
    let Ok(mut peers) = context.shared.peers.lock() else {
        return;
    };
    for sighting in sightings {
        match sighting {
            Sighting::Seen { addrs, .. } => {
                for addr in discovery::dedup(addrs) {
                    if !peers.addrs.contains(&addr) {
                        peers.addrs.push(addr);
                    }
                }
            }
            // Ушедший экземпляр не даёт адреса напрямую — он просто перестанет
            // отвечать, и его устройство сойдёт с онлайна по сроку.
            Sighting::Gone { .. } => {}
        }
    }
}

fn probe_round(context: &Context) {
    let now = Instant::now();
    let addrs = match context.shared.peers.lock() {
        Ok(peers) => peers
            .addrs
            .iter()
            .filter(|addr| {
                peers
                    .strangers
                    .get(addr)
                    .map_or(true, |until| now >= *until)
            })
            .copied()
            .collect::<Vec<_>>(),
        Err(_) => return,
    };

    for addr in addrs {
        if context.stale() {
            return;
        }
        match probe(context, addr) {
            Some(device_id) => {
                if let Ok(mut peers) = context.shared.peers.lock() {
                    peers.strangers.remove(&addr);
                }
                mark_online(context, &device_id);
            }
            None => {
                if let Ok(mut peers) = context.shared.peers.lock() {
                    peers
                        .strangers
                        .insert(addr, Instant::now() + context.settings.stranger_backoff);
                }
            }
        }
    }
}

/// Позвонить по адресу и выяснить, свой ли там.
fn probe(context: &Context, addr: SocketAddr) -> Option<String> {
    let identity = context.identity()?;
    let mut session = dial(context, addr, &identity, Mode::Trusted).ok()?;

    // Доверие решает ключ. Ни адрес, ни то, что устройство вообще ответило, в
    // этом решении не участвуют (§2.1).
    let device = context
        .with_core(|core| core.authorize_peer(&session.peer_signing).ok().flatten())
        .flatten()?;

    match session.channel.request(&Msg::Ping) {
        Ok(Msg::Pong) => Some(device.device_id),
        _ => None,
    }
}

fn dial(
    context: &Context,
    addr: SocketAddr,
    identity: &NetIdentity,
    mode: Mode,
) -> CoreResult<handshake::Established> {
    let stream = TcpStream::connect_timeout(&addr, context.settings.connect_timeout)?;
    let stream = channel::with_timeouts(stream, context.settings.io_timeout)?;
    handshake::initiate(
        stream,
        &identity.keypair,
        mode,
        handshake::ephemeral_secret()?,
    )
}

// ---------------------------------------------------------------------------
// Сопряжение по сети
// ---------------------------------------------------------------------------

/// Спросить у соседа пейлоад сеанса. `None` — это не он.
fn ask_for_payload(context: &Context, addr: SocketAddr, code: &str) -> Option<String> {
    let identity = context.identity()?;
    let mut session = dial(context, addr, &identity, Mode::Pairing).ok()?;

    // В канал уходит обязательство, а не код: собеседник ещё не подтверждён, а
    // шесть символов — это как раз то, что мог бы подбирать он сам.
    let commitment = pairing::code_commitment(&session.transcript, code);
    match session.channel.request(&Msg::PairingLookup { commitment }) {
        Ok(Msg::PairingOffer { payload }) => Some(payload),
        _ => None,
    }
}

/// Доложить показавшей код стороне, что сопряжение состоялось (§2.2).
fn announce_pairing(context: &Context, announcement: &PairingAnnouncement) {
    for attempt in 0..ANNOUNCE_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(ANNOUNCE_PAUSE);
        }
        if pairing::is_expired(&announcement.expires_at, chrono::Utc::now()) {
            return;
        }

        let addrs = context
            .shared
            .peers
            .lock()
            .map(|peers| peers.addrs.clone())
            .unwrap_or_default();
        for addr in addrs {
            if deliver_pairing(context, addr, announcement).is_some() {
                return;
            }
        }
    }
    // Не дошло: мы сопряжены, а вторая сторона про нас не знает. Человек увидит
    // это в списке устройств и покажет код заново — врать ему об успехе доставки
    // мы не станем, потому что рассказывать об этом нечем: команда уже ответила.
}

fn deliver_pairing(
    context: &Context,
    addr: SocketAddr,
    announcement: &PairingAnnouncement,
) -> Option<()> {
    let identity = context.identity()?;
    let mut session = dial(context, addr, &identity, Mode::Pairing).ok()?;

    // Тот ли это экран. Ключ сеанса приехал внеполосно, в QR, и сосед, который
    // не тот, половину ECDH под него подставить не может (§2.2). Без этой
    // проверки обязательство по коду уехало бы кому попало.
    if session.peer_ecdh != announcement.session_public {
        return None;
    }

    let public = identity.keypair.public_key();
    let mut agreement_public = [0u8; HALF_PUBLIC_LEN];
    agreement_public.copy_from_slice(&public[HALF_PUBLIC_LEN..]);

    let message = Msg::PairingComplete {
        commitment: pairing::code_commitment(&session.transcript, &announcement.code),
        device_id: identity.device_id.clone(),
        name: identity.host.name.clone(),
        kind: identity.host.kind,
        agreement_public,
    };
    match session.channel.request(&message) {
        Ok(Msg::PairingAck) => Some(()),
        _ => None,
    }
}
