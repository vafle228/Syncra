//! Шифрованный канал поверх TCP (§3.2).
//!
//! После рукопожатия по проводу не ходит ничего открытого: каждое сообщение —
//! отдельный AEAD-кадр под XChaCha20-Poly1305, тем же примитивом, что
//! запечатывает пароли в файле (§8.1). Своего здесь не изобретается ничего,
//! кроме дисциплины nonce — а она описана ниже.

use std::io::{Read, Write};
use std::net::TcpStream;

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::error::{CoreError, CoreResult};

use super::wire::{self, Msg};

/// Ключ одного направления.
pub type DirectionKey = Zeroizing<[u8; 32]>;

/// AAD всех кадров канала.
///
/// Своё пространство, как у `field_aad` и `syncra:device:secret`: кадр канала
/// нельзя выдать за запечатанное поле записи и наоборот.
const FRAME_AAD: &[u8] = b"syncra:net:frame:v1";

const NONCE_LEN: usize = 24;

/// Канал: сокет плюс по ключу и счётчику на каждое направление.
///
/// **Почему ключи разные.** Nonce здесь — просто счётчик, и он начинается с
/// нуля у обеих сторон. С одним общим ключом первое сообщение инициатора и
/// первое сообщение отвечающего шифровались бы одним и тем же ключом с одним и
/// тем же nonce — а это ровно тот случай, который ломает потоковый шифр целиком.
/// Разные ключи делают повтор счётчика безобидным по построению, а не по
/// договорённости.
pub struct Channel {
    stream: TcpStream,
    send_key: DirectionKey,
    recv_key: DirectionKey,
    send_counter: u64,
    recv_counter: u64,
}

impl Channel {
    pub fn new(stream: TcpStream, send_key: DirectionKey, recv_key: DirectionKey) -> Self {
        Self {
            stream,
            send_key,
            recv_key,
            send_counter: 0,
            recv_counter: 0,
        }
    }

    pub fn send(&mut self, msg: &Msg) -> CoreResult<()> {
        let nonce = Self::nonce(self.send_counter)?;
        let sealed = cipher(&self.send_key)?
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &msg.encode()?,
                    aad: FRAME_AAD,
                },
            )
            .map_err(|_| wire::malformed())?;

        self.send_counter += 1;
        wire::write_frame(&mut self.stream, &sealed)
    }

    pub fn recv(&mut self) -> CoreResult<Msg> {
        let sealed = wire::read_frame(&mut self.stream)?;
        let nonce = Self::nonce(self.recv_counter)?;

        // Порядок кадров — часть договора: счётчик получателя не «подбирается»
        // под пришедший кадр, а идёт по порядку. Переставленный, повторённый или
        // выброшенный кадр поэтому не расшифровывается вовсе.
        let plaintext = cipher(&self.recv_key)?
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &sealed,
                    aad: FRAME_AAD,
                },
            )
            .map_err(|_| wire::malformed())?;

        self.recv_counter += 1;
        Msg::decode(&plaintext)
    }

    /// Обмен «спросил — получил ответ» одним движением.
    pub fn request(&mut self, msg: &Msg) -> CoreResult<Msg> {
        self.send(msg)?;
        self.recv()
    }

    /// Nonce из счётчика: восемь байт счётчика в хвосте, остальное — нули.
    ///
    /// Переполнение `u64` недостижимо (это больше кадров, чем пройдёт по сети за
    /// время жизни вселенной), но обрабатывается отказом, а не молчаливым
    /// повтором: повторённый nonce — это раскрытый ключевой поток.
    fn nonce(counter: u64) -> CoreResult<[u8; NONCE_LEN]> {
        if counter == u64::MAX {
            return Err(CoreError::internal("Соединение исчерпало себя."));
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce[NONCE_LEN - 8..].copy_from_slice(&counter.to_be_bytes());
        Ok(nonce)
    }
}

fn cipher(key: &DirectionKey) -> CoreResult<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(&key[..])
        .map_err(|_| CoreError::internal("Ключ канала повреждён."))
}

/// Мелочь, которой не место в двух файлах сразу: сокет с таймаутами на чтение и
/// запись. Без них зависший собеседник держит поток вечно, а поток — сокет.
pub fn with_timeouts(stream: TcpStream, timeout: std::time::Duration) -> CoreResult<TcpStream> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// Прочитать приветствие открытым текстом — до того, как канал существует.
pub fn read_plain(stream: &mut impl Read) -> CoreResult<Vec<u8>> {
    wire::read_frame(stream)
}

/// Записать приветствие открытым текстом.
pub fn write_plain(stream: &mut impl Write, body: &[u8]) -> CoreResult<()> {
    wire::write_frame(stream, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn pair() -> (Channel, Channel) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();

        let a: DirectionKey = Zeroizing::new([1u8; 32]);
        let b: DirectionKey = Zeroizing::new([2u8; 32]);
        (
            Channel::new(client, a.clone(), b.clone()),
            Channel::new(server, b, a),
        )
    }

    #[test]
    fn messages_travel_both_ways() {
        let (mut client, mut server) = pair();

        client.send(&Msg::Ping).unwrap();
        assert_eq!(server.recv().unwrap(), Msg::Ping);

        server.send(&Msg::Pong).unwrap();
        assert_eq!(client.recv().unwrap(), Msg::Pong);
    }

    #[test]
    fn the_counter_moves_with_every_frame() {
        let (mut client, mut server) = pair();

        // Три кадра подряд в одну сторону: если бы счётчик стоял на месте, они
        // шифровались бы одним nonce.
        for _ in 0..3 {
            client.send(&Msg::Ping).unwrap();
        }
        for _ in 0..3 {
            assert_eq!(server.recv().unwrap(), Msg::Ping);
        }
        assert_eq!(client.send_counter, 3);
        assert_eq!(server.recv_counter, 3);
    }

    #[test]
    fn the_same_message_never_looks_the_same_on_the_wire() {
        let a: DirectionKey = Zeroizing::new([1u8; 32]);
        let cipher = cipher(&a).unwrap();

        let first = cipher
            .encrypt(
                XNonce::from_slice(&Channel::nonce(0).unwrap()),
                Payload {
                    msg: b"ping",
                    aad: FRAME_AAD,
                },
            )
            .unwrap();
        let second = cipher
            .encrypt(
                XNonce::from_slice(&Channel::nonce(1).unwrap()),
                Payload {
                    msg: b"ping",
                    aad: FRAME_AAD,
                },
            )
            .unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn a_frame_from_the_wrong_direction_does_not_open() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();

        // Сервер по ошибке настроен на ОДИН ключ в обе стороны: то, что прислал
        // клиент своим ключом отправки, у него не открывается.
        let a: DirectionKey = Zeroizing::new([1u8; 32]);
        let b: DirectionKey = Zeroizing::new([2u8; 32]);
        let mut client = Channel::new(client, a, b.clone());
        let mut server = Channel::new(server, b.clone(), b);

        client.send(&Msg::Ping).unwrap();
        assert!(server.recv().is_err());
    }

    #[test]
    fn a_repeated_frame_does_not_open_twice() {
        let (mut client, mut server) = pair();

        client.send(&Msg::Ping).unwrap();
        assert_eq!(server.recv().unwrap(), Msg::Ping);

        // Тот же кадр, записанный в провод второй раз: у получателя счётчик уже
        // ушёл вперёд, и повтор не расшифровывается.
        let replay = cipher(&Zeroizing::new([1u8; 32]))
            .unwrap()
            .encrypt(
                XNonce::from_slice(&Channel::nonce(0).unwrap()),
                Payload {
                    msg: &Msg::Ping.encode().unwrap(),
                    aad: FRAME_AAD,
                },
            )
            .unwrap();
        wire::write_frame(&mut client.stream, &replay).unwrap();
        assert!(server.recv().is_err());
    }
}
