/// TCP 通信殻。
///
/// ## レイヤー構造
///
/// ```text
/// ┌─────────────────────────────────────────────────────┐
/// │ トランスポート共通（将来も変わらない部分）              │
/// │   NetMessage  — メッセージ語彙（何を送るか）           │
/// │   NetEvent    — 受信イベント型                        │
/// │   Connection  — 公開 API: .send() / .events          │
/// ├─────────────────────────────────────────────────────┤
/// │ TCP 固有（将来 WS や Unix socket へ差し替える部分）     │
/// │   Connection::listen / connect — 接続確立             │
/// │   reader_loop / send — 4 byte 長さプレフィックス       │
/// └─────────────────────────────────────────────────────┘
/// ```
///
/// 別トランスポートへ移行する場合: `NetMessage` / `NetEvent` と
/// `Connection` の公開シグネチャ（`.send()` / `.events`）は保持したまま、
/// `listen` / `connect` の確立ロジックと、`reader_loop` / `send` の
/// フレーミング部分（`// [TCP framing]` コメント箇所）を置き換える。
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use serde::{Deserialize, Serialize};

use protocol::{BoardHash, Commitment, Nonce};

// ─── トランスポート共通: メッセージ語彙・イベント型 ──────────────────────────

/// ワイヤー上を流れるメッセージ
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NetMessage {
    /// 接続直後のハンドシェイク
    GameStart {
        /// 0=先手, 1=後手
        side: u8,
        /// SHA-256(secret) の hex 文字列
        secret_hash: String,
    },
    /// commit フェーズ
    Commit {
        commitment: String, // hex
    },
    /// reveal フェーズ
    Reveal {
        action_usi: String,
        nonce: String,      // hex
        board_hash: String, // hex
    },
    /// ack フェーズ
    Ack,
    /// 再接続ハンドシェイク
    Reconnect {
        /// 秘密の本体（hex エンコード）— 相手が SHA-256 して照合する
        secret: String,
        /// 申告する現局面ハッシュ
        resume_hash: String,
    },
    /// 接続直後のバージョン交渉（ハンドシェイク第一関門）
    VersionHello {
        rule_major: u32,
        rule_minor: u32,
        protocol: u32,
    },
    /// プロトコル違反・ハッシュ不一致によるアボート
    Abort { reason: String },
}

/// net スレッドからメインスレッドへのイベント
#[derive(Debug)]
pub enum NetEvent {
    Message(NetMessage),
    Disconnected,
}

// ─── トランスポート共通: 接続ハンドル（公開 API のみ共通; 実装は TCP 固有）──

/// 接続ハンドル。
///
/// 呼び出し側から見た公開 API（`.send()` / `.events`）はトランスポート共通。
/// 内部の `TcpStream` と受信スレッドが TCP 固有の実装。
pub struct Connection {
    stream: TcpStream,
    pub events: Receiver<NetEvent>,
}

// ─── トランスポート共通: バージョン交渉エラー ─────────────────────────────────

/// 版交渉の失敗を表す型（殻側のラッパー）
#[derive(Debug)]
pub enum NegotiationError {
    /// 版の不一致・不正応答・タイムアウト（protocol クレートの純粋判定結果）
    Negotiation(protocol::NegotiationOutcome),
    /// 送受信中の IO エラー
    Io(std::io::Error),
}

impl From<std::io::Error> for NegotiationError {
    fn from(e: std::io::Error) -> Self {
        NegotiationError::Io(e)
    }
}

// ─── TCP 固有: 接続確立 ───────────────────────────────────────────────────────

impl Connection {
    /// 指定ポートで待ち受けて最初の接続を受け入れる（ブロッキング）
    pub fn listen(port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        let (stream, _addr) = listener.accept()?;
        Self::from_stream(stream)
    }

    /// 指定アドレスへ接続する（ブロッキング）
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        Self::from_stream(stream)
    }

    fn from_stream(stream: TcpStream) -> std::io::Result<Self> {
        let reader = stream.try_clone()?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || reader_loop(reader, tx));
        Ok(Self { stream, events: rx })
    }

    /// 版交渉を実行する（ハンドシェイクの第一関門）。
    ///
    /// 自分の版を送り、相手の版を受け取って完全一致を確認する。
    /// タイムアウトの計時はここ（殻）、判定は `protocol::negotiate_versions`（純粋）。
    /// 成功すると `VersionCleared` を返し、呼び出し側は認証フェーズへ進める。
    pub fn perform_version_negotiation(
        &mut self,
    ) -> Result<protocol::VersionCleared, NegotiationError> {
        use protocol::{negotiate_versions, PeerVersionResponse, VersionTuple, MY_VERSION};
        use std::time::Duration;

        let mine = MY_VERSION;

        // [transport-agnostic] 自分の版を送信
        self.send(&NetMessage::VersionHello {
            rule_major: mine.rule.0,
            rule_minor: mine.rule.1,
            protocol: mine.protocol,
        })?;

        // [transport-agnostic] 相手の版を受信（10 秒タイムアウト）
        let peer = match self.events.recv_timeout(Duration::from_secs(10)) {
            Ok(NetEvent::Message(NetMessage::VersionHello {
                rule_major,
                rule_minor,
                protocol,
            })) => PeerVersionResponse::Version(VersionTuple {
                rule: (rule_major, rule_minor),
                protocol,
            }),
            Ok(_) => PeerVersionResponse::Invalid, // 別メッセージ or 切断
            Err(_) => PeerVersionResponse::Timeout, // タイムアウト or チャネル閉鎖
        };

        negotiate_versions(&mine, peer).map_err(NegotiationError::Negotiation)
    }

    /// メッセージを1つ送信する（公開 API はトランスポート共通）
    pub fn send(&mut self, msg: &NetMessage) -> std::io::Result<()> {
        let body = serde_json::to_vec(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // [TCP framing] 4 byte big-endian 長さプレフィックス
        let len = (body.len() as u32).to_be_bytes();
        self.stream.write_all(&len)?;
        self.stream.write_all(&body)?;
        self.stream.flush()
    }
}

// ─── TCP 固有: フレーミングと受信ループ ──────────────────────────────────────

fn reader_loop(mut stream: TcpStream, tx: Sender<NetEvent>) {
    loop {
        // [TCP framing] 4 byte big-endian 長さプレフィックスでメッセージ境界を判定
        let mut len_buf = [0u8; 4];
        if stream.read_exact(&mut len_buf).is_err() {
            let _ = tx.send(NetEvent::Disconnected);
            return;
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len == 0 || len > 1_048_576 {
            let _ = tx.send(NetEvent::Disconnected);
            return;
        }
        let mut body = vec![0u8; len];
        if stream.read_exact(&mut body).is_err() {
            let _ = tx.send(NetEvent::Disconnected);
            return;
        }
        match serde_json::from_slice::<NetMessage>(&body) {
            Ok(msg) => {
                if tx.send(NetEvent::Message(msg)).is_err() {
                    return;
                }
            }
            Err(_) => {
                let _ = tx.send(NetEvent::Disconnected);
                return;
            }
        }
    }
}

// ─── トランスポート共通: hex ユーティリティ（NetMessage の JSON フィールド用）──

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

pub fn commitment_to_hex(c: &Commitment) -> String {
    to_hex(&c.0)
}

pub fn commitment_from_hex(s: &str) -> Option<Commitment> {
    let v = from_hex(s)?;
    if v.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&v);
    Some(Commitment(arr))
}

pub fn nonce_to_hex(n: &Nonce) -> String {
    to_hex(&n.0)
}

pub fn nonce_from_hex(s: &str) -> Option<Nonce> {
    let v = from_hex(s)?;
    if v.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&v);
    Some(Nonce(arr))
}

pub fn board_hash_to_hex(h: &BoardHash) -> String {
    to_hex(&h.0)
}

pub fn board_hash_from_hex(s: &str) -> Option<BoardHash> {
    let v = from_hex(s)?;
    if v.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&v);
    Some(BoardHash(arr))
}
