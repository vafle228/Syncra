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
//! слушатель  ── accept ──► рукопожатие ──► доверенный: ping/pong, обмен,
//!                                                        peer_found
//!                                      └─► сопряжение: показать пейлоад,
//!                                                      принять «я тебя записал»
//! обходчик   ── mDNS ──► адреса ──► проба доверенным рукопожатием ──► peer_found
//!                              └──► назрел круг обмена ──► манифест и дифф
//! ```
//!
//! Круг обмена ведёт [`sync`]: там чередование реплик и нарезка на порции, здесь
//! — когда его затевать и что показывать человеку, пока он идёт.

pub mod channel;
pub mod discovery;
pub mod handshake;
pub mod sync;
pub mod wire;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use crate::crypto::DeviceKeypair;
use crate::error::{CoreError, CoreResult};
use crate::model::{
    now_iso, Device, DeviceKind, HostDevice, IsoDateTime, PairingHandshake, PairingResult,
    PeerFound, RecordConflict, RecordId, SyncPhase, SyncStatus,
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
    /// Приехавшая версия разошлась с местной (`conflict_raised`,
    /// `contract.ts:1324`). Немедленной реакции не требует: конфликт ждёт в
    /// списке столько, сколько нужно (§5.5).
    ConflictRaised(Box<RecordConflict>),
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
    /// Как часто затевать круг обмена сам, без просьбы человека (§5.3).
    pub sync_interval: Duration,
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
            // «Следующая попытка через минуту, вручную можно раньше» — обещание
            // макета, и цифра здесь именно из него. Чаще незачем: обмен идёт
            // диффом и в тишине не везёт ничего, но каждый круг — это дозвон до
            // соседа и работа с его диском.
            sync_interval: Duration::from_secs(60),
        }
    }
}

/// Как часто слушатель просыпается посмотреть, не пора ли уходить.
const ACCEPT_POLL: Duration = Duration::from_millis(200);

/// Сколько разговоров узел ведёт одновременно.
///
/// Поток обработчика заводится ДО всякой аутентификации — до рукопожатия, до
/// проверки подписи, до `authorize_peer`, — и каждый такой поток это ECDH, свой
/// стек и до мегабайта под кадр (`wire::MAX_FRAME`). Слушатель стоит на
/// `0.0.0.0`, поэтому позвонить может кто угодно, и без потолка тысяча сокетов
/// от одного хоста в сети превращается в тысячу потоков ядра.
///
/// Десятки, а не тысячи: соседей у человека единицы, и даже они звонят по
/// одному соединению за круг.
pub const MAX_HANDLERS: usize = 32;

/// Сколько ждать неаутентифицированный сокет — от `accept` до подписи.
///
/// Отдельно от `io_timeout` и заметно короче: полноценный разговор бывает
/// медленным (на той стороне диск и мьютекс ядра), а рукопожатие — это две
/// посылки и ECDH. Тот, кто позвонил и молчит, не должен занимать место в
/// [`MAX_HANDLERS`] пять секунд.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Шаг обходчика. Не то же, что `probe_interval`: обходчик просыпается чаще,
/// чтобы вовремя заметить остановку и внеочередную просьбу поискать.
const WALK_STEP: Duration = Duration::from_millis(250);

/// Сколько раз доставка «я тебя записал» пробует достучаться. Дальше сроку
/// оффера всё равно конец.
const ANNOUNCE_ATTEMPTS: u32 = 6;

/// Пауза между попытками доставки.
const ANNOUNCE_PAUSE: Duration = Duration::from_secs(2);

/// Как часто обходчик переспрашивает ядро про «ждут отправки» и «последний
/// обмен».
///
/// Не каждый шаг: это запрос к БД под мьютексом ядра, а секунда — та же
/// зернистость, с какой работает сторож автоблокировки. Правка записи попадёт
/// в индикатор не позже чем через неё.
const FACTS_STEP: Duration = Duration::from_secs(1);

// ---------------------------------------------------------------------------
// Общее состояние
// ---------------------------------------------------------------------------

/// Взять замок общего состояния, не считаясь с отравой.
///
/// [`Shared`] — это счётчики и кэши. Паника в потоке, который держал такой
/// замок, делает их устаревшими, а не неверными, и продолжать с ними честнее,
/// чем ослепнуть: при `.lock().ok()` отравленный мьютекс означал бы «соседей
/// ноль» и «статус рассылать некому» **навсегда** — то есть UI спокойно
/// показывал бы нормальную работу вместо поломки.
///
/// Мьютекс ЯДРА так брать нельзя и не берётся: отравленное ядро правда нельзя
/// считать целым — см. [`Context::with_core`] и `commands.rs::core!`.
fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
struct Peers {
    /// Что видно в сети. Кто это — ещё неизвестно.
    addrs: Vec<SocketAddr>,
    /// Адрес → до какого момента в него не звонить (там не Syncra или не наш).
    strangers: HashMap<SocketAddr, Instant>,
    /// `device_id` → когда доверенное устройство последний раз отозвалось.
    online: HashMap<String, Instant>,
}

/// Почему последняя попытка не удалась.
///
/// Имя пира лежит рядом с текстом, потому что макет в фазе `error` пишет
/// «Соединение с «X» оборвалось»: имя переживает ошибку и гаснет вместе с ней.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Failure {
    message: String,
    peer_name: Option<String>,
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
    /// Внеочередная просьба обменяться — она же (`sync_now`).
    sync_requested: AtomicBool,
    /// С кем прямо сейчас идёт круг. Оно же и есть фаза `syncing`: двух правд о
    /// том, идёт ли обмен, быть не должно.
    syncing: Mutex<Option<String>>,
    /// Почему сеть не поднялась или сорвался обмен. Непусто только в фазе `error`.
    failure: Mutex<Option<Failure>>,
    /// Кэш ответов ядра для статуса.
    ///
    /// Кэш, а не запрос на месте: статус пересчитывается из сетевых потоков, в
    /// том числе изнутри разговора с соседом, и лезть за ним в ядро значило бы
    /// брать нереентрантный мьютекс, который, может быть, уже взят.
    pending: Mutex<Vec<RecordId>>,
    last_sync_at: Mutex<Option<IsoDateTime>>,
    /// Последний разосланный статус — чтобы не слать одно и то же дважды.
    last_status: Mutex<Option<SyncStatus>>,
    events: Sender<CoreEvent>,
    port: AtomicU64,
    /// Сколько обработчиков входящих соединений живёт прямо сейчас.
    handlers: AtomicUsize,
}

impl Shared {
    /// Забыть, кто был признан чужим.
    ///
    /// Зовётся, когда таблица доверия изменилась сопряжением: вердикт «это не
    /// наш» вынесен по старому составу `devices` и после нового сопряжения
    /// просто неверен. Без этого только что сопряжённое (или возвращённое из
    /// отзыва) устройство ждало бы своей пробы до конца отката.
    fn forget_strangers(&self) {
        held(&self.peers).strangers.clear();
    }

    fn failure(&self) -> Option<Failure> {
        held(&self.failure).clone()
    }

    fn fail_with(&self, message: impl Into<String>, peer_name: Option<String>) {
        *held(&self.failure) = Some(Failure {
            message: message.into(),
            peer_name,
        });
    }

    fn forget_failure(&self) {
        *held(&self.failure) = None;
    }

    fn announce(&self, event: CoreEvent) {
        // Событие — не отчёт об успехе: некому доставить, значит некому. Ронять
        // из-за этого сетевой поток нельзя (та же логика, что в `announce`
        // оболочки).
        let _ = self.events.send(event);
    }
}

/// Право вести круг обмена — ровно одно на узел.
///
/// Оба устройства ходят друг к другу сами, и встречные круги неизбежны. Второй
/// круг ничего не испортит (применение идёт транзакцией, а правило свежести
/// идемпотентно), но проделает ту же работу впустую и заставит индикатор
/// мигать. Проще договориться: кто первый занял — тот и ведёт.
///
/// Освобождается на выходе из области видимости, в том числе если круг оборвался
/// ошибкой или поток ушёл по смене поколения.
struct Busy<'a> {
    shared: &'a Shared,
}

impl<'a> Busy<'a> {
    fn claim(shared: &'a Shared, peer_name: &str) -> Option<Self> {
        {
            let mut slot = held(&shared.syncing);
            if slot.is_some() {
                return None;
            }
            *slot = Some(peer_name.to_owned());
        }
        // Круг начался — прошлая неудача больше не описывает происходящее
        // (`mock/index.ts::doStartSync` гасит `message` ровно здесь же).
        shared.forget_failure();
        Some(Self { shared })
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        *held(&self.shared.syncing) = None;
    }
}

/// Место в списке одновременных разговоров.
///
/// Занимается ДО того, как заведён поток: считать уже запущенные значило бы
/// сначала запустить тысячу. Освобождается в [`Drop`], поэтому ни ошибка, ни
/// паника внутри `serve` его не заклинивают.
struct Handler(Arc<Shared>);

impl Handler {
    fn claim(shared: &Arc<Shared>) -> Option<Self> {
        if shared.handlers.fetch_add(1, Ordering::SeqCst) >= MAX_HANDLERS {
            shared.handlers.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self(Arc::clone(shared)))
    }
}

impl Drop for Handler {
    fn drop(&mut self) {
        self.0.handlers.fetch_sub(1, Ordering::SeqCst);
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
            sync_requested: AtomicBool::new(false),
            syncing: Mutex::new(None),
            failure: Mutex::new(None),
            pending: Mutex::new(Vec::new()),
            last_sync_at: Mutex::new(None),
            last_status: Mutex::new(None),
            events,
            port: AtomicU64::new(0),
            handlers: AtomicUsize::new(0),
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
        let unlocked = match self.core.lock() {
            Ok(core) => core.is_unlocked(),
            // Отравленное ядро — это поломка, а не «заперто»: спокойный ноль в
            // индикаторе соврал бы про неё, а сеть с таким ядром всё равно не
            // работает (`Context::with_core` вернёт `None` каждому потоку).
            Err(_) => return self.fail_poisoned(),
        };

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
        let mut peers = held(&self.shared.peers);
        if !peers.addrs.contains(&addr) {
            peers.addrs.push(addr);
        }
        peers.strangers.remove(&addr);
    }

    /// Текущее состояние синхронизации целиком.
    ///
    /// Спрашивают его командой, из потока UI: замок ядра здесь свободен, и
    /// «ждут отправки» можно освежить прямо сейчас, а не ждать шага обходчика.
    pub fn status(&self) -> SyncStatus {
        self.refresh_facts();
        compute_status(&self.shared, Instant::now())
    }

    /// «Синхронизировать сейчас» (F10).
    ///
    /// Не ждать своей минуты: обходчик обойдёт соседей и проведёт круг обмена на
    /// ближайшем же шаге. Ответ команды при этом круга НЕ ждёт — обмен идёт по
    /// сети, а команда должна вернуться сразу; о том, чем он кончился, расскажет
    /// событие `sync_status` (так же устроен и `mock/index.ts::syncNow`).
    pub fn sync_now(&self) -> SyncStatus {
        *held(&self.shared.searching_until) = Some(Instant::now() + self.settings.search_window);
        self.shared.forget_failure();
        self.shared.probe_requested.store(true, Ordering::SeqCst);
        self.shared.sync_requested.store(true, Ordering::SeqCst);
        self.refresh_facts();
        publish_status(&self.shared, Instant::now())
    }

    /// Освежить кэш «ждут отправки» и «последний обмен».
    fn refresh_facts(&self) {
        let _ = self.lock_core(|core| refresh_facts(&self.shared, core));
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
        let mut result = self.lock_core(|core| core.confirm_pairing(session_id))??;
        self.shared.forget_strangers();
        let Some(announcement) = self.lock_core(|core| core.take_pairing_announcement())? else {
            return Ok(result);
        };

        // Первый проход — здесь и сейчас, уже без замка ядра. Сосед, чей код
        // только что прочитали, почти наверняка на связи, а первичный перенос
        // записей человек ждёт именно от этой кнопки: «сопряжено, перенесено
        // столько-то» — это ответ команды, а не новость через минуту.
        let context = self.context();
        match deliver_once(&context, &announcement) {
            Some(addr) => {
                result.records_transferred = i64::from(exchange_at(&context, addr).unwrap_or(0));
            }
            // Не достучались: сопряжение всё равно состоялось, а доставку и
            // перенос доделает фон. Ответ команды его не ждёт — держать человека
            // на экране сопряжения ради шести попыток по две секунды нельзя.
            None => {
                std::thread::spawn(move || announce_pairing(&context, &announcement));
            }
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

        self.shared.forget_failure();
        *held(&self.shared.searching_until) = Some(Instant::now() + self.settings.search_window);
        self.shared.port.store(u64::from(port), Ordering::SeqCst);
        self.shared.running.store(true, Ordering::SeqCst);
        // «Последний обмен» переживает не только замок, но и перезапуск: он
        // лежит в `meta`, и прочитать его надо ровно один раз, здесь.
        self.refresh_facts();

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

        *held(&self.shared.peers) = Peers::default();
        *held(&self.shared.searching_until) = None;
        // Запертое хранилище — это спокойный ноль, а не «обмен не удался»:
        // рассказывать про соседей и ждущие записи ему нечем и незачем.
        self.shared.forget_failure();
        *held(&self.shared.syncing) = None;
        held(&self.shared.pending).clear();
        *held(&self.shared.last_sync_at) = None;
        publish_status(&self.shared, Instant::now());
    }

    /// Ядро отравлено: сеть с ним не работает, и молчать об этом нельзя.
    ///
    /// Сначала опустить сеть (иначе её потоки останутся крутиться впустую),
    /// потом зажечь `error`: `stop` гасит неудачу, и порядок здесь обратный
    /// был бы порядком, в котором сообщение стирается сразу после появления.
    fn fail_poisoned(&self) {
        if self.shared.running.load(Ordering::SeqCst) {
            self.stop();
        }
        self.fail("Ядро в несогласованном состоянии — синхронизация остановлена.");
    }

    fn fail(&self, message: impl Into<String>) {
        self.shared.fail_with(message, None);
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
        let addrs = held(&context.shared.peers).addrs.clone();

        for addr in addrs {
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
    let failure = shared.failure();

    if !shared.running.load(Ordering::SeqCst) {
        // Сеть не поднялась — это ошибка, о которой человеку надо сказать.
        // Просто заперто — спокойный ноль: `stop` гасит и неудачу, и кэш.
        return match failure {
            Some(failure) => SyncStatus {
                phase: SyncPhase::Error,
                peer_name: failure.peer_name,
                message: Some(failure.message),
                ..SyncStatus::idle()
            },
            None => SyncStatus::idle(),
        };
    }

    let online = held(&shared.peers).online.len() as u32;
    let syncing = held(&shared.syncing).clone();
    let searching = online == 0
        && syncing.is_none()
        && failure.is_none()
        && held(&shared.searching_until).is_some_and(|until| now < until);

    // Порядок — это приоритет: то, что происходит прямо сейчас, важнее того,
    // чем кончился прошлый круг.
    let (phase, peer_name, message) = match (syncing, failure) {
        (Some(peer), _) => (SyncPhase::Syncing, Some(peer), None),
        (None, Some(failure)) => (SyncPhase::Error, failure.peer_name, Some(failure.message)),
        (None, None) if searching => (SyncPhase::Searching, None, None),
        (None, None) => (SyncPhase::Idle, None, None),
    };

    SyncStatus {
        // `idle` с найденными соседями и пустым списком ожидающих — это и есть
        // «всё сошлось»: остальные пять видов индикатора UI выводит сам.
        phase,
        peers_online: online,
        peer_name,
        pending_records: held(&shared.pending).clone(),
        last_sync_at: held(&shared.last_sync_at).clone(),
        message,
    }
}

/// Перечитать у ядра то, что статус сам знать не может.
///
/// Зовётся с уже взятым замком ядра — и ничего сетевого внутри не делает.
fn refresh_facts(shared: &Shared, core: &mut Core) {
    if let Ok(pending) = core.pending_records() {
        *held(&shared.pending) = pending;
    }
    if let Ok(at) = core.last_sync_at() {
        *held(&shared.last_sync_at) = at;
    }
}

/// Разослать статус, если он изменился.
///
/// Уходит ПОЛНЫЙ статус, а не дельта: подписка живёт не всё время работы окна, и
/// собранное по кусочкам состояние разъехалось бы с ядром на первом же
/// пропущенном событии (`contract.ts:1290`).
fn publish_status(shared: &Shared, now: Instant) -> SyncStatus {
    let status = compute_status(shared, now);

    let changed = {
        let mut last = held(&shared.last_status);
        let changed = last.as_ref() != Some(&status);
        if changed {
            *last = Some(status.clone());
        }
        changed
    };
    if changed {
        shared.announce(CoreEvent::SyncStatus(Box::new(status.clone())));
    }
    status
}

/// Устройство отозвалось: подвинуть `last_seen_at`, сказать про него, если оно
/// только что появилось.
fn mark_online(context: &Context, device_id: &str) {
    let fresh = held(&context.shared.peers)
        .online
        .insert(device_id.to_owned(), Instant::now())
        .is_none();

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
    let dropped = {
        let mut peers = held(&context.shared.peers);
        let before = peers.online.len();
        peers
            .online
            .retain(|_, seen| now.duration_since(*seen) < context.settings.peer_ttl);
        before != peers.online.len()
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
                // Сверх потолка — закрыть молча. Ответить «занято» значило бы
                // потратить на позвонившего ещё немного себя, а он пока никто.
                let Some(handler) = Handler::claim(&context.shared) else {
                    drop(stream);
                    continue;
                };
                let context = context.clone();
                // Каждое соединение — свой поток: разговор с одним соседом не
                // должен мешать принять второго.
                std::thread::spawn(move || {
                    let _handler = handler;
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
    // Поколение прошлое — хранилище уже заперто, и разговаривать нечем. ECDH
    // ради того, чтобы тут же положить трубку, не нужен никому.
    if context.stale() {
        return Ok(());
    }

    // Принятый сокет наследует неблокирующий режим слушателя — а разговор с ним
    // ведётся обычными чтениями с таймаутом.
    stream.set_nonblocking(false)?;
    // До подписи — короткий срок, после неё — обычный: см. `HANDSHAKE_TIMEOUT`.
    let first = context.settings.io_timeout.min(HANDSHAKE_TIMEOUT);
    let stream = channel::with_timeouts(stream, first)?;

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
    let mut established = handshake::accept(stream, &identity.keypair, secret_for)?;
    // Собеседник подтверждён подписью — дальше разговор идёт обычными сроками.
    established
        .channel
        .set_timeouts(context.settings.io_timeout)?;

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

    // Круг обмена принадлежит соединению: его состояние живёт здесь, локальной
    // переменной, и умирает вместе с разговором.
    let mut round = sync::Round::default();
    let mut busy: Option<Busy<'_>> = None;

    while !context.stale() {
        match session.channel.recv() {
            Ok(Msg::Ping) => {
                session.channel.send(&Msg::Pong)?;
                // Разговор идёт — значит устройство на связи прямо сейчас.
                // Без этого отметка протухла бы по сроку на стороне, которая
                // только отвечает и сама никому не звонит.
                mark_online(context, &device.device_id);
            }
            Ok(message @ (Msg::SyncManifest { .. } | Msg::SyncFetch | Msg::SyncBatch { .. })) => {
                if busy.is_none() {
                    // Встречный круг уже идёт — второй сейчас только повторил бы
                    // ту же работу. Отвечаем кадром, а не обрывом: обрыв зажёг бы
                    // у соседа «обмен не удался» там, где всё в порядке.
                    let Some(claimed) = Busy::claim(&context.shared, &device.name) else {
                        session.channel.send(&Msg::SyncBusy)?;
                        break;
                    };
                    busy = Some(claimed);
                    publish_status(&context.shared, Instant::now());
                }

                match sync::answer(context, &mut round, &device.device_id, message) {
                    Ok((reply, done)) => {
                        session.channel.send(&reply)?;
                        mark_online(context, &device.device_id);
                        if done {
                            finish_round(context, busy.take());
                            // Соединение переживает круг, а состояние круга —
                            // нет: второй круг по той же трубе должен начинаться
                            // с чистого манифеста, а не с накопленного прошлым.
                            round = sync::Round::default();
                        }
                    }
                    Err(error) => {
                        context
                            .shared
                            .fail_with(error.message, Some(device.name.clone()));
                        drop(busy.take());
                        publish_status(&context.shared, Instant::now());
                        break;
                    }
                }
            }
            // Всё остальное — либо конец разговора, либо непонятный кадр; в обоих
            // случаях вешаем трубку.
            _ => break,
        }
    }
    Ok(())
}

/// Круг доехал до конца: отпустить право вести, освежить кэш и рассказать.
fn finish_round(context: &Context, busy: Option<Busy<'_>>) {
    drop(busy);
    context.with_core(|core| refresh_facts(&context.shared, core));
    publish_status(&context.shared, Instant::now());
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
                    // Записи поедут ближайшим кругом. Здесь их не перенести:
                    // адреса СЛУШАТЕЛЯ соседа мы не знаем — в сокете сопряжения
                    // его эфемерный клиентский порт, — и позвонить ему пока
                    // некуда. Поэтому в событии честный ноль.
                    context.shared.sync_requested.store(true, Ordering::SeqCst);
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
    // Первый круг — сразу: хранилище только что отперли, и за время, пока оно
    // было заперто, у соседа могло измениться что угодно.
    let mut next_sync = Instant::now();
    let mut next_facts = Instant::now();

    while !context.stale() {
        let now = Instant::now();
        absorb(context, discovery.drain());

        let asked = context.shared.probe_requested.swap(false, Ordering::SeqCst);
        let exchange =
            context.shared.sync_requested.swap(false, Ordering::SeqCst) || now >= next_sync;
        if asked || exchange || now >= next_probe {
            probe_round(context, exchange);
            next_probe = Instant::now() + context.settings.probe_interval;
            if exchange {
                next_sync = Instant::now() + context.settings.sync_interval;
            }
        }
        expire_online(context, Instant::now());

        if now >= next_facts {
            context.with_core(|core| refresh_facts(&context.shared, core));
            next_facts = Instant::now() + FACTS_STEP;
        }
        publish_status(&context.shared, Instant::now());

        std::thread::sleep(WALK_STEP);
    }
    discovery.shutdown();
}

fn absorb(context: &Context, sightings: Vec<Sighting>) {
    let mut peers = held(&context.shared.peers);
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

fn probe_round(context: &Context, exchange: bool) {
    let now = Instant::now();
    let addrs = {
        let peers = held(&context.shared.peers);
        peers
            .addrs
            .iter()
            .filter(|addr| {
                peers
                    .strangers
                    .get(addr)
                    .map_or(true, |until| now >= *until)
            })
            .copied()
            .collect::<Vec<_>>()
    };

    for addr in addrs {
        if context.stale() {
            return;
        }
        match visit(context, addr, exchange) {
            Some(device_id) => {
                held(&context.shared.peers).strangers.remove(&addr);
                mark_online(context, &device_id);
            }
            None => {
                held(&context.shared.peers)
                    .strangers
                    .insert(addr, Instant::now() + context.settings.stranger_backoff);
            }
        }
    }
}

/// Позвонить по адресу, выяснить, свой ли там, и — если назрело — обменяться.
///
/// Круг идёт по тому же соединению, что и проба: рукопожатие уже сделано, а
/// второй дозвон до того же соседа ради того же разговора — это лишний ECDH и
/// лишний сокет.
fn visit(context: &Context, addr: SocketAddr, exchange: bool) -> Option<String> {
    let identity = context.identity()?;
    let mut session = dial(context, addr, &identity, Mode::Trusted).ok()?;

    // Доверие решает ключ. Ни адрес, ни то, что устройство вообще ответило, в
    // этом решении не участвуют (§2.1).
    let device = context
        .with_core(|core| core.authorize_peer(&session.peer_signing).ok().flatten())
        .flatten()?;

    match session.channel.request(&Msg::Ping) {
        Ok(Msg::Pong) => {}
        _ => return None,
    }

    if exchange {
        // Отметка до круга, а не после: круг бывает долгим, а «устройство на
        // связи» стало правдой уже сейчас.
        mark_online(context, &device.device_id);
        exchange_with(context, &mut session, &device);
    }
    Some(device.device_id)
}

/// Провести круг обмена по готовому соединению, показывая, что происходит.
///
/// Возвращает, сколько записей переехало в обе стороны, или `None`, если круг
/// не состоялся: право вести занято встречным кругом либо обмен сорвался.
fn exchange_with(
    context: &Context,
    session: &mut handshake::Established,
    device: &Device,
) -> Option<u32> {
    let busy = Busy::claim(&context.shared, &device.name)?;
    publish_status(&context.shared, Instant::now());

    let moved = sync::run_round(context, session, &device.device_id);
    match moved {
        // Собеседник занят встречным кругом — ни успеха, ни ошибки.
        Ok(None) => {
            drop(busy);
            publish_status(&context.shared, Instant::now());
            None
        }
        Ok(Some(moved)) => {
            finish_round(context, Some(busy));
            Some(moved)
        }
        Err(error) => {
            // Данные целы: не доехал дифф. Так это и объясняет макет, и красный
            // здесь не нужен — следующая попытка придёт сама.
            context
                .shared
                .fail_with(error.message, Some(device.name.clone()));
            drop(busy);
            publish_status(&context.shared, Instant::now());
            None
        }
    }
}

/// Дозвониться до доверенного соседа по адресу и провести круг.
fn exchange_at(context: &Context, addr: SocketAddr) -> Option<u32> {
    let identity = context.identity()?;
    let mut session = dial(context, addr, &identity, Mode::Trusted).ok()?;
    let device = context
        .with_core(|core| core.authorize_peer(&session.peer_signing).ok().flatten())
        .flatten()?;

    mark_online(context, &device.device_id);
    exchange_with(context, &mut session, &device)
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

/// Повторять доставку «я тебя записал», пока не дойдёт (§2.2).
///
/// Первый проход делает сам `confirm_pairing`, поэтому здесь только повторы — и
/// каждый начинается с паузы. Дошло — тем же заходом переносим записи: сосед
/// только что узнал про нас, и ждать своей минуты незачем.
fn announce_pairing(context: &Context, announcement: &PairingAnnouncement) {
    for _ in 1..ANNOUNCE_ATTEMPTS {
        std::thread::sleep(ANNOUNCE_PAUSE);
        if pairing::is_expired(&announcement.expires_at, chrono::Utc::now()) {
            return;
        }
        if let Some(addr) = deliver_once(context, announcement) {
            exchange_at(context, addr);
            return;
        }
    }
    // Не дошло: мы сопряжены, а вторая сторона про нас не знает. Человек увидит
    // это в списке устройств и покажет код заново — врать ему об успехе доставки
    // мы не станем, потому что рассказывать об этом нечем: команда уже ответила.
}

/// Один проход по известным адресам. Возвращает тот, по которому дошло.
fn deliver_once(context: &Context, announcement: &PairingAnnouncement) -> Option<SocketAddr> {
    let addrs = held(&context.shared.peers).addrs.clone();

    addrs
        .into_iter()
        .find(|addr| deliver_pairing(context, *addr, announcement).is_some())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Отравить мьютекс так, как это случается в жизни: паникой в потоке,
    /// который его держал.
    fn poison<T: Send + Sync>(mutex: &Mutex<T>) {
        std::thread::scope(|scope| {
            let _ = scope
                .spawn(|| {
                    let _guard = mutex.lock().unwrap();
                    panic!("поток упал с замком в руках");
                })
                .join();
        });
        assert!(mutex.is_poisoned(), "мьютекс должен был отравиться");
    }

    /// Общее состояние и приёмник его событий: приёмник должен жить до конца
    /// теста, иначе `announce` начнёт отваливаться и проверять мы будем не то.
    fn shared() -> (Arc<Shared>, Receiver<CoreEvent>) {
        let (events, inbox) = mpsc::channel();
        let shared = Arc::new(Shared {
            generation: AtomicU64::new(0),
            running: AtomicBool::new(true),
            peers: Mutex::new(Peers::default()),
            searching_until: Mutex::new(None),
            probe_requested: AtomicBool::new(false),
            sync_requested: AtomicBool::new(false),
            syncing: Mutex::new(None),
            failure: Mutex::new(None),
            pending: Mutex::new(Vec::new()),
            last_sync_at: Mutex::new(None),
            last_status: Mutex::new(None),
            events,
            port: AtomicU64::new(0),
            handlers: AtomicUsize::new(0),
        });
        (shared, inbox)
    }

    #[test]
    fn a_poisoned_lock_does_not_turn_the_status_into_a_lie() {
        let (shared, _inbox) = shared();
        held(&shared.peers)
            .online
            .insert("телефон".to_owned(), Instant::now());
        held(&shared.pending).push("r-1".to_owned());

        poison(&shared.peers);
        poison(&shared.pending);
        poison(&shared.last_status);

        // Соседи на месте, ждущие записи на месте — состояние после паники
        // устарело, но не стало неверным, и показывать вместо него спокойный
        // ноль значило бы соврать про поломку.
        let status = compute_status(&shared, Instant::now());
        assert_eq!(status.peers_online, 1);
        assert_eq!(status.pending_records, vec!["r-1".to_owned()]);

        // И рассылка статуса не онемела: `last_status` под отравой читается.
        assert_eq!(publish_status(&shared, Instant::now()), status);
        assert!(held(&shared.last_status).is_some());
    }

    #[test]
    fn a_poisoned_failure_slot_still_takes_a_message() {
        let (shared, _inbox) = shared();
        poison(&shared.failure);

        shared.fail_with("обмен не удался", Some("Телефон".to_owned()));
        assert_eq!(
            shared.failure().map(|failure| failure.message),
            Some("обмен не удался".to_owned())
        );

        shared.forget_failure();
        assert!(shared.failure().is_none());
    }
}
