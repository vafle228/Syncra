//! Рукопожатие: ECDH, транскрипт и подпись (§3.2, §2.1).
//!
//! Схема стандартная и намеренно скучная: эфемерный ECDH на X25519 даёт общий
//! секрет, из него выводятся два ключа канала, и уже ВНУТРИ канала каждая
//! сторона подписывает транскрипт своим долговременным ключом Ed25519.
//!
//! **Почему подпись, а не «ключ в devices лежит — значит свой».** Наличие
//! публичного ключа ничего не доказывает: он публичный. Доказывает подпись
//! именно этого разговора — транскрипт входит в неё целиком, поэтому чужую
//! подпись из другого соединения предъявить нельзя.
//!
//! **Почему эфемерный ECDH, а не долговременный X25519.** Долговременный дал бы
//! один и тот же общий секрет на все разговоры за всё время: укравший файл
//! хранилища и подобравший мастер-пароль расшифровал бы весь записанный ранее
//! трафик. Эфемерный ключ умирает вместе с соединением.
//!
//! **Чего эта схема не скрывает.** Публичный ключ подписи едет в приветствии
//! открытым текстом, и наблюдатель в той же сети видит, что вот это устройство
//! — то же самое, что и вчера. Спрятать это можно, но не бесплатно; §11 спека
//! держит вопрос открытым, и MVP 1 его не закрывает.

use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::crypto::{self, DeviceKeypair};
use crate::error::{CoreError, CoreResult};

use super::channel::{Channel, DirectionKey};
use super::wire::{self, Hello, Mode, Msg, Role, HALF_PUBLIC_LEN, HELLO_NONCE_LEN, SIGNATURE_LEN};

/// Пространство подписи рукопожатия.
///
/// Отличается от `pairing::SIGNATURE_DOMAIN` намеренно и с самого S2: подпись
/// пейлоада QR нельзя предъявить как подпись рукопожатия и наоборот.
const AUTH_DOMAIN: &[u8] = b"syncra:handshake:v1:auth";

/// Пространство транскрипта.
const TRANSCRIPT_DOMAIN: &[u8] = b"syncra:handshake:v1";

/// Пространство вывода ключей канала.
const KEY_DOMAIN: &[u8] = b"syncra:handshake:v1:key";

/// Метки направлений. Инициатор шифрует своим `i2r` и расшифровывает `r2i`;
/// отвечающий — наоборот.
const LABEL_I2R: &[u8] = b"i2r";
const LABEL_R2I: &[u8] = b"r2i";

/// Собеседник не тот, за кого себя выдал.
fn not_authenticated() -> CoreError {
    CoreError::internal("Устройство в сети не подтвердило, что это оно.")
}

/// Установленное соединение.
pub struct Established {
    pub channel: Channel,
    /// Подтверждённый подписью ключ собеседника. Именно по нему — и только по
    /// нему — принимается решение о доверии (§2.1).
    pub peer_signing: [u8; HALF_PUBLIC_LEN],
    /// Половина ECDH, которую собеседник назвал в приветствии. Прочитавшая QR
    /// сторона сверяет её с ключом сеанса из пейлоада.
    pub peer_ecdh: [u8; HALF_PUBLIC_LEN],
    /// Отпечаток разговора. Из него считается обязательство по коду сопряжения.
    pub transcript: [u8; 32],
    pub mode: Mode,
}

/// Начать рукопожатие (мы позвонили).
pub fn initiate(
    stream: std::net::TcpStream,
    keypair: &DeviceKeypair,
    mode: Mode,
    ecdh_secret: StaticSecret,
) -> CoreResult<Established> {
    shake(stream, keypair, mode, ecdh_secret, Role::Initiator)
}

/// Ответить на рукопожатие (позвонили нам).
///
/// `ecdh_secret` приходит аргументом, а не заводится внутри: сторона, которая
/// прямо сейчас показывает код сопряжения, подставляет сюда одноразовый ключ
/// СЕАНСА из своего пейлоада. Тогда прочитавший QR сверяет половину ECDH с той,
/// что была в коде, и канал оказывается привязан к отсканированному сеансу —
/// подставиться посередине больше нечем (§2.2).
pub fn accept(
    stream: std::net::TcpStream,
    keypair: &DeviceKeypair,
    mode_secret: impl FnOnce(Mode) -> CoreResult<StaticSecret>,
) -> CoreResult<Established> {
    shake_responder(stream, keypair, mode_secret)
}

/// Свежий одноразовый секрет ECDH.
pub fn ephemeral_secret() -> CoreResult<StaticSecret> {
    Ok(StaticSecret::from(crypto::random_bytes::<32>()?))
}

// ---------------------------------------------------------------------------
// Общая часть
// ---------------------------------------------------------------------------

fn hello_of(
    keypair: &DeviceKeypair,
    mode: Mode,
    role: Role,
    ecdh_secret: &StaticSecret,
) -> CoreResult<Hello> {
    let public = keypair.public_key();
    let mut signing_public = [0u8; HALF_PUBLIC_LEN];
    signing_public.copy_from_slice(&public[..HALF_PUBLIC_LEN]);

    Ok(Hello {
        role,
        mode,
        signing_public,
        ecdh_public: PublicKey::from(ecdh_secret).to_bytes(),
        nonce: crypto::random_bytes::<HELLO_NONCE_LEN>()?,
    })
}

fn shake(
    mut stream: std::net::TcpStream,
    keypair: &DeviceKeypair,
    mode: Mode,
    ecdh_secret: StaticSecret,
    role: Role,
) -> CoreResult<Established> {
    let own = hello_of(keypair, mode, role, &ecdh_secret)?;

    // Инициатор говорит первым и только потом слушает; отвечающий — наоборот.
    // Иначе оба ждут друг друга на пустом сокете.
    let peer = {
        super::channel::write_plain(&mut stream, &own.encode())?;
        let bytes = super::channel::read_plain(&mut stream)?;
        Hello::decode(&bytes, role.opposite())?
    };
    finish(stream, keypair, own, peer, ecdh_secret, role)
}

fn shake_responder(
    mut stream: std::net::TcpStream,
    keypair: &DeviceKeypair,
    mode_secret: impl FnOnce(Mode) -> CoreResult<StaticSecret>,
) -> CoreResult<Established> {
    let bytes = super::channel::read_plain(&mut stream)?;
    let peer = Hello::decode(&bytes, Role::Initiator)?;

    // Режим называет позвонивший, и от режима зависит, какой у нас будет ключ
    // ECDH: в режиме сопряжения это ключ сеанса показанного кода.
    let ecdh_secret = mode_secret(peer.mode)?;
    let own = hello_of(keypair, peer.mode, Role::Responder, &ecdh_secret)?;
    super::channel::write_plain(&mut stream, &own.encode())?;

    finish(stream, keypair, own, peer, ecdh_secret, Role::Responder)
}

fn finish(
    stream: std::net::TcpStream,
    keypair: &DeviceKeypair,
    own: Hello,
    peer: Hello,
    ecdh_secret: StaticSecret,
    role: Role,
) -> CoreResult<Established> {
    // Режимы обязаны совпасть: разговор про сопряжение нельзя начать как
    // доверенный и продолжить как чужой.
    if own.mode != peer.mode {
        return Err(wire::malformed());
    }

    let (hello_i, hello_r) = match role {
        Role::Initiator => (&own, &peer),
        Role::Responder => (&peer, &own),
    };
    let transcript = transcript(hello_i, hello_r);

    let shared = Zeroizing::new(
        ecdh_secret
            .diffie_hellman(&PublicKey::from(peer.ecdh_public))
            .to_bytes(),
    );
    let (send_label, recv_label) = match role {
        Role::Initiator => (LABEL_I2R, LABEL_R2I),
        Role::Responder => (LABEL_R2I, LABEL_I2R),
    };
    let mut channel = Channel::new(
        stream,
        channel_key(&shared, &transcript, send_label),
        channel_key(&shared, &transcript, recv_label),
    );

    // Подпись едет ВНУТРИ канала: наблюдателю не видно даже того, что стороны
    // друг друга признали.
    let signature = keypair
        .signing_key()
        .sign(&signed_bytes(role, &transcript))
        .to_bytes();

    // Порядок тот же, что и у приветствий, и по той же причине.
    let peer_auth = match role {
        Role::Initiator => {
            channel.send(&Msg::Auth { signature })?;
            channel.recv()?
        }
        Role::Responder => {
            let received = channel.recv()?;
            channel.send(&Msg::Auth { signature })?;
            received
        }
    };
    let Msg::Auth {
        signature: peer_signature,
    } = peer_auth
    else {
        return Err(wire::malformed());
    };
    verify_auth(
        &peer.signing_public,
        role.opposite(),
        &transcript,
        &peer_signature,
    )?;

    Ok(Established {
        channel,
        peer_signing: peer.signing_public,
        peer_ecdh: peer.ecdh_public,
        transcript,
        mode: own.mode,
    })
}

/// Отпечаток разговора: домен и оба приветствия целиком, в порядке ролей.
///
/// В нём есть всё, о чём стороны договорились открытым текстом, — версия,
/// режим, оба публичных ключа и обе случайности. Подменить что-то одно и
/// сохранить подпись поэтому нельзя.
fn transcript(hello_initiator: &Hello, hello_responder: &Hello) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(TRANSCRIPT_DOMAIN);
    for hello in [hello_initiator, hello_responder] {
        let bytes = hello.encode();
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(&bytes);
    }
    hasher.finalize().into()
}

/// Ключ одного направления.
///
/// Хеш, а не HKDF: extract здесь нечего делать — общий секрет X25519 и так
/// равномерен, — а expand на один блок это ровно один вызов SHA-256. Домен и
/// метка направления разделяют выходы; транскрипт привязывает ключ к разговору.
fn channel_key(shared: &[u8; 32], transcript: &[u8; 32], label: &[u8]) -> DirectionKey {
    let mut hasher = Sha256::new();
    hasher.update(KEY_DOMAIN);
    hasher.update(label);
    hasher.update(shared);
    hasher.update(transcript);
    Zeroizing::new(hasher.finalize().into())
}

/// Что именно подписывается. Роль входит в подпись, поэтому подпись отвечающего
/// нельзя вернуть инициатору как его собственную.
fn signed_bytes(role: Role, transcript: &[u8; 32]) -> Vec<u8> {
    let mut signed = Vec::with_capacity(AUTH_DOMAIN.len() + 1 + transcript.len());
    signed.extend_from_slice(AUTH_DOMAIN);
    signed.push(role.as_byte());
    signed.extend_from_slice(transcript);
    signed
}

fn verify_auth(
    signing_public: &[u8; HALF_PUBLIC_LEN],
    role: Role,
    transcript: &[u8; 32],
    signature: &[u8; SIGNATURE_LEN],
) -> CoreResult<()> {
    let verifying = VerifyingKey::from_bytes(signing_public).map_err(|_| not_authenticated())?;
    verifying
        .verify(
            &signed_bytes(role, transcript),
            &Signature::from_bytes(signature),
        )
        .map_err(|_| not_authenticated())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transcript_covers_everything_said_in_the_open() {
        let base = Hello {
            role: Role::Initiator,
            mode: Mode::Trusted,
            signing_public: [1u8; HALF_PUBLIC_LEN],
            ecdh_public: [2u8; HALF_PUBLIC_LEN],
            nonce: [3u8; HELLO_NONCE_LEN],
        };
        let responder = Hello {
            role: Role::Responder,
            ..base.clone()
        };
        let original = transcript(&base, &responder);

        // Любая подмена в любом приветствии меняет отпечаток.
        for changed in [
            Hello {
                mode: Mode::Pairing,
                ..base.clone()
            },
            Hello {
                signing_public: [9u8; HALF_PUBLIC_LEN],
                ..base.clone()
            },
            Hello {
                ecdh_public: [9u8; HALF_PUBLIC_LEN],
                ..base.clone()
            },
            Hello {
                nonce: [9u8; HELLO_NONCE_LEN],
                ..base.clone()
            },
        ] {
            assert_ne!(original, transcript(&changed, &responder));
        }
    }

    #[test]
    fn the_two_directions_get_different_keys() {
        let shared = [4u8; 32];
        let transcript = [5u8; 32];

        let i2r = channel_key(&shared, &transcript, LABEL_I2R);
        let r2i = channel_key(&shared, &transcript, LABEL_R2I);
        assert_ne!(&i2r[..], &r2i[..]);

        // ...и другой разговор с тем же общим секретом даёт другие ключи.
        let elsewhere = channel_key(&shared, &[6u8; 32], LABEL_I2R);
        assert_ne!(&i2r[..], &elsewhere[..]);
    }

    #[test]
    fn a_signature_of_one_role_does_not_pass_as_the_other() {
        let pair = DeviceKeypair::generate().unwrap();
        let transcript = [7u8; 32];

        let public = pair.public_key();
        let mut signing_public = [0u8; HALF_PUBLIC_LEN];
        signing_public.copy_from_slice(&public[..HALF_PUBLIC_LEN]);

        let signature = pair
            .signing_key()
            .sign(&signed_bytes(Role::Initiator, &transcript))
            .to_bytes();

        assert!(verify_auth(&signing_public, Role::Initiator, &transcript, &signature).is_ok());
        // Отражённое рукопожатие: ту же подпись предъявляют как подпись отвечающего.
        assert!(verify_auth(&signing_public, Role::Responder, &transcript, &signature).is_err());
        // ...и в другом разговоре она тоже не годится.
        assert!(verify_auth(&signing_public, Role::Initiator, &[8u8; 32], &signature).is_err());
    }

    #[test]
    fn someone_elses_signature_does_not_pass() {
        let pair = DeviceKeypair::generate().unwrap();
        let impostor = DeviceKeypair::generate().unwrap();
        let transcript = [7u8; 32];

        let public = pair.public_key();
        let mut signing_public = [0u8; HALF_PUBLIC_LEN];
        signing_public.copy_from_slice(&public[..HALF_PUBLIC_LEN]);

        let signature = impostor
            .signing_key()
            .sign(&signed_bytes(Role::Initiator, &transcript))
            .to_bytes();
        assert!(verify_auth(&signing_public, Role::Initiator, &transcript, &signature).is_err());
    }
}
