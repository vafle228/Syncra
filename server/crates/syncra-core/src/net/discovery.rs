//! Обнаружение устройств в локальной сети (§5.1, §8.3 «Discovery»).
//!
//! # Почему mDNS, а не ARP
//!
//! §5.1 спека называет ARP «предварительно», §8.1 разрешает «ARP/mDNS». Выбран
//! mDNS, и вот по каким причинам — записаны здесь, чтобы к вопросу больше не
//! возвращаться:
//!
//! 1. **ARP требует сырых сокетов**, а значит прав администратора. Менеджер
//!    паролей, который просит поднять себе привилегии ради поиска соседей, —
//!    это плохой размен.
//! 2. **ARP не несёт ни имени службы, ни порта.** Он отвечает на вопрос «какой
//!    MAC у этого IP», а нам нужно «есть ли рядом Syncra и на каком порту». По
//!    ARP пришлось бы стучаться во все адреса подсети подряд.
//! 3. **На iOS (MVP 2) ARP недоступен вовсе**, а mDNS там штатный механизм —
//!    спека сама упоминает `NSLocalNetworkUsageDescription` и multicast
//!    entitlement (§8.4). Строить обнаружение дважды незачем.
//!
//! # Чего в анонсе нет
//!
//! Ни идентификатора устройства, ни публичного ключа, ни имени машины. Имя
//! экземпляра — случайный токен, новый на каждое отпирание; в TXT только версия.
//!
//! mDNS слушает вся сеть, включая гостей и соседей за стенкой. Стабильный
//! идентификатор в широковещании — это возможность следить за устройством, и при
//! этом он ничего не доказывает: назваться чужим `device_id` может кто угодно.
//! Кто перед нами, выясняет рукопожатие, и только оно (§2.1: адрес помогает
//! найти устройство, но не даёт основания ему доверять).

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::crypto;
use crate::error::{CoreError, CoreResult};

/// Служба, которую Syncra анонсирует и ищет.
pub const SERVICE_TYPE: &str = "_syncra._tcp.local.";

/// Версия анонса — единственное, что лежит в TXT.
const TXT_VERSION: (&str, &str) = ("v", "1");

/// Длина случайного имени экземпляра, байт до кодирования.
const INSTANCE_TOKEN_LEN: usize = 8;

/// Что увидело обнаружение.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sighting {
    /// Экземпляр службы отозвался и назвал адреса. Кто это — ещё неизвестно.
    Seen {
        instance: String,
        addrs: Vec<SocketAddr>,
    },
    /// Экземпляр ушёл из сети.
    Gone { instance: String },
}

/// Ошибка mDNS наружу не пересказывается: её текст несёт имена интерфейсов и
/// адреса, а `message` уходит прямо в UI.
fn mdns_failed() -> CoreError {
    CoreError::internal("Не удалось начать поиск устройств в локальной сети.")
}

/// Обнаружение — либо настоящее, либо выключенное.
///
/// Выключенное нужно тестам: настоящий мультикаст в `cargo test` зависит от
/// брандмауэра и от того, что ещё крутится в сети у запускающего, — то есть
/// проверял бы не ядро. Транспорт при этом проверяется полностью: адреса просто
/// приходят узлу готовыми, как приходил бы результат поиска.
pub enum Discovery {
    Off,
    Mdns(Box<Mdns>),
}

pub struct Mdns {
    daemon: ServiceDaemon,
    /// Полное имя своего экземпляра — по нему снимают анонс и узнают себя же в
    /// собственном мультикасте.
    fullname: String,
    events: mdns_sd::Receiver<ServiceEvent>,
}

impl Discovery {
    /// Анонсировать себя и начать просмотр.
    pub fn start(port: u16) -> CoreResult<Self> {
        let daemon = ServiceDaemon::new().map_err(|_| mdns_failed())?;

        let token = instance_token()?;
        let host = format!("syncra-{token}.local.");
        let info = ServiceInfo::new(SERVICE_TYPE, &token, &host, (), port, &[TXT_VERSION][..])
            .map_err(|_| mdns_failed())?
            // Адреса берёт сам демон и обновляет их сам: список интерфейсов машины
            // меняется на ходу (Wi-Fi, VPN, док-станция), и ядру это знать незачем.
            .enable_addr_auto();

        let fullname = info.get_fullname().to_owned();
        daemon.register(info).map_err(|_| mdns_failed())?;

        let events = daemon.browse(SERVICE_TYPE).map_err(|_| mdns_failed())?;
        Ok(Self::Mdns(Box::new(Mdns {
            daemon,
            fullname,
            events,
        })))
    }

    /// Забрать всё, что накопилось, не блокируя поток.
    pub fn drain(&self) -> Vec<Sighting> {
        let Self::Mdns(mdns) = self else {
            return Vec::new();
        };

        let mut seen = Vec::new();
        while let Ok(event) = mdns.events.try_recv() {
            match event {
                ServiceEvent::ServiceResolved(service) => {
                    // Свой же анонс: мультикаст возвращается и нам тоже.
                    // Соединяться с самим собой незачем — рукопожатие всё равно
                    // не признало бы нас соседом, но и звонить себе не будем.
                    if service.fullname == mdns.fullname {
                        continue;
                    }
                    let addrs = service
                        .addresses
                        .iter()
                        .map(|scoped| SocketAddr::new(scoped.to_ip_addr(), service.port))
                        .collect::<Vec<_>>();
                    if !addrs.is_empty() {
                        seen.push(Sighting::Seen {
                            instance: service.fullname.clone(),
                            addrs,
                        });
                    }
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    seen.push(Sighting::Gone { instance: fullname });
                }
                _ => {}
            }
        }
        seen
    }

    /// Снять анонс и остановить демон.
    ///
    /// Зовётся при запирании хранилища: запертое устройство в сети не
    /// участвует — ни отвечать, ни называть себя ему нечем.
    pub fn shutdown(self) {
        let Self::Mdns(mdns) = self else {
            return;
        };
        // Оба вызова возвращают канал подтверждения, и оба ответа нам не нужны:
        // мы останавливаемся, а не проверяем, как прошла остановка.
        let _ = mdns.daemon.unregister(&mdns.fullname);
        let _ = mdns.daemon.shutdown();
    }
}

/// Имя экземпляра: случайный токен в шестнадцатеричном виде.
///
/// Случайность берётся у ОС, как и везде в ядре. Предсказуемое имя экземпляра
/// связывало бы два появления одного устройства в сети между собой — то есть
/// делало бы ровно то, ради отказа от чего идентификатора здесь и нет.
fn instance_token() -> CoreResult<String> {
    Ok(crypto::random_bytes::<INSTANCE_TOKEN_LEN>()?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Годится ли адрес, чтобы в него звонить.
///
/// Синхронизация живёт в пределах одной локальной сети (§5.1), и адреса вне её
/// отсеиваются здесь — не ради безопасности (доверие решает ключ), а чтобы не
/// тратить таймауты на заведомо чужое.
pub fn is_worth_calling(addr: &SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V4(v4) => !v4.is_unspecified() && !v4.is_multicast() && !v4.is_broadcast(),
        IpAddr::V6(v6) => !v6.is_unspecified() && !v6.is_multicast(),
    }
}

/// Отсеять повторы, сохранив порядок: один хост часто отвечает с нескольких
/// интерфейсов, и звонить в каждый смысла нет.
pub fn dedup(addrs: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut seen = HashSet::new();
    addrs
        .into_iter()
        .filter(is_worth_calling)
        .filter(|addr| seen.insert(*addr))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn an_instance_token_is_random_and_hex() {
        let first = instance_token().unwrap();
        assert_eq!(first.len(), INSTANCE_TOKEN_LEN * 2);
        assert!(first.chars().all(|char| char.is_ascii_hexdigit()));
        // Два отпирания подряд не связываются друг с другом по имени.
        assert_ne!(first, instance_token().unwrap());
    }

    #[test]
    fn junk_addresses_are_not_worth_a_timeout() {
        let port = 4000;
        assert!(!is_worth_calling(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port
        )));
        assert!(!is_worth_calling(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)),
            port
        )));
        assert!(!is_worth_calling(&SocketAddr::new(
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            port
        )));
        assert!(is_worth_calling(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 7)),
            port
        )));
        // Loopback годится: на нём стоят тесты и два экземпляра на одной машине.
        assert!(is_worth_calling(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            port
        )));
    }

    #[test]
    fn one_host_on_two_interfaces_is_called_once() {
        let a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 7)), 4000);
        let b = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)), 4000);
        let junk = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4000);

        assert_eq!(dedup([a, b, a, junk, b]), vec![a, b]);
    }

    #[test]
    fn discovery_that_is_off_sees_nothing_and_shuts_down_quietly() {
        let off = Discovery::Off;
        assert!(off.drain().is_empty());
        off.shutdown();
    }
}
