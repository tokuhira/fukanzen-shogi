/// TCP 通信殻。
///
/// ## レイヤー構造
///
/// ```text
/// ┌─────────────────────────────────────────────────────┐
/// │ トランスポート共通（将来も変わらない部分）              │
/// │   WireMessage — メッセージ語彙（protocol クレート正本） │
/// │   NetEvent    — 受信イベント型                        │
/// │   Connection  — 公開 API: .send() / .events          │
/// ├─────────────────────────────────────────────────────┤
/// │ TCP 固有（将来 WS や Unix socket へ差し替える部分）     │
/// │   Connection::listen / connect — 接続確立             │
/// │   reader_loop / send — 4 byte 長さプレフィックス       │
/// └─────────────────────────────────────────────────────┘
/// ```
///
/// 版交渉は `ClientSession` が hello（`WireMessage::Hello`）の中で行う
/// （`protocol::negotiate_versions` を core が呼ぶ）。net.rs はワイヤの
/// 送受信・framing のみを担い、プロトコル意味を持たない。
///
/// 別トランスポートへ移行する場合: `WireMessage` / `NetEvent` と
/// `Connection` の公開シグネチャ（`.send()` / `.events`）は保持したまま、
/// `listen` / `connect` の確立ロジックと、`reader_loop` / `send` の
/// フレーミング部分（`// [TCP framing]` コメント箇所）を置き換える。
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use protocol::WireMessage;

// ─── トランスポート共通: イベント型 ───────────────────────────────────────────

/// net スレッドからメインスレッドへのイベント
#[derive(Debug)]
pub enum NetEvent {
    Message(WireMessage),
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

    /// メッセージを1つ送信する（公開 API はトランスポート共通）
    pub fn send(&mut self, msg: &WireMessage) -> std::io::Result<()> {
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
        match serde_json::from_slice::<WireMessage>(&body) {
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
