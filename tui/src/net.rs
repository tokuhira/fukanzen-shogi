/// TCP 通信殻。
///
/// - 4 バイト big-endian 長さプレフィックス + serde_json ボディ
/// - 受信スレッドが `mpsc::Sender<NetEvent>` へイベントを送る
/// - メインスレッドは `try_recv` でノンブロッキングに受け取る
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use serde::{Deserialize, Serialize};

use protocol::{BoardHash, Commitment, Nonce};

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
    /// プロトコル違反・ハッシュ不一致によるアボート
    Abort {
        reason: String,
    },
}

/// net スレッドからメインスレッドへのイベント
#[derive(Debug)]
pub enum NetEvent {
    Message(NetMessage),
    Disconnected,
}

/// TCP ストリームを包んだ接続ハンドル。
///
/// 受信スレッドは `events` チャネルにメッセージを送り続ける。
/// 送信は `send` メソッドで行う（メインスレッドから呼ぶ）。
pub struct Connection {
    stream: TcpStream,
    pub events: Receiver<NetEvent>,
}

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

    /// メッセージを送信する
    pub fn send(&mut self, msg: &NetMessage) -> std::io::Result<()> {
        let body = serde_json::to_vec(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let len = (body.len() as u32).to_be_bytes();
        self.stream.write_all(&len)?;
        self.stream.write_all(&body)?;
        self.stream.flush()
    }
}

fn reader_loop(mut stream: TcpStream, tx: Sender<NetEvent>) {
    loop {
        // 4バイトの長さヘッダを読む
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

// ─── hex ユーティリティ ───────────────────────────────────────────────────────

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
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
    if v.len() != 32 { return None; }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&v);
    Some(Commitment(arr))
}

pub fn nonce_to_hex(n: &Nonce) -> String {
    to_hex(&n.0)
}

pub fn nonce_from_hex(s: &str) -> Option<Nonce> {
    let v = from_hex(s)?;
    if v.len() != 32 { return None; }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&v);
    Some(Nonce(arr))
}

pub fn board_hash_to_hex(h: &BoardHash) -> String {
    to_hex(&h.0)
}

pub fn board_hash_from_hex(s: &str) -> Option<BoardHash> {
    let v = from_hex(s)?;
    if v.len() != 32 { return None; }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&v);
    Some(BoardHash(arr))
}
