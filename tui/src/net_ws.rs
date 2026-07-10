/// WebSocket 通信殻（クラウド参加）。
///
/// `tui/src/net.rs` の TCP 殻と同じ公開 API（`send(&WireMessage)` / `events`）を、
/// 同期 `tungstenite`＋rustls の上で実装する。DO（Durable Object）の部屋
/// （`wss://…/room/<部屋キー>`）へ接続し、対局チャネルの `WireMessage` と
/// DO のシステムメッセージ（side 割り当て・切断・再接続・満室）を
/// `NetEvent` へ分類して surface する。
///
/// tungstenite の `WebSocket<Stream>` は読み書きを分割できないため、
/// `Arc<Mutex<>>` で reader スレッドとメインの送信を安全に共有する。
/// reader は下層 TCP に読み取りタイムアウトを設定し、ロックを長く持たない
/// （タイムアウトで定期的に手放し、送信側に機会を与える）。
use std::io;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tungstenite::{
    handshake::HandshakeError, stream::MaybeTlsStream, Error as WsProtoError, Message, WebSocket,
};

use engine::types::Side;
use protocol::WireMessage;

use crate::net::{DoSystemMsg, NetEvent};

/// 本番 DO の WebSocket ベース URL。
pub const CLOUD_SERVER_URL: &str = "wss://fukanzen-shogi-ws.tokuhira.workers.dev";

/// 受信ループのブロッキング読み取りタイムアウト。この間隔でロックを手放し、
/// 送信側（メインスレッド）が書き込めるようにする。
const READ_TIMEOUT: Duration = Duration::from_millis(200);

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

#[derive(Debug)]
pub enum WsError {
    /// 部屋が満員（DO が WS ハンドシェイクを HTTP 403 で拒否）。
    RoomFull,
    BadUrl,
    Handshake(String),
    Io(io::Error),
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsError::RoomFull => write!(f, "この部屋は満室です"),
            WsError::BadUrl => write!(f, "接続先 URL が不正です"),
            WsError::Handshake(msg) => write!(f, "WebSocket 接続に失敗しました: {}", msg),
            WsError::Io(e) => write!(f, "通信エラー: {}", e),
        }
    }
}

impl From<io::Error> for WsError {
    fn from(e: io::Error) -> Self {
        WsError::Io(e)
    }
}

pub struct WsConnection {
    ws: Arc<Mutex<Sock>>,
    pub events: Receiver<NetEvent>,
}

impl WsConnection {
    /// `server_url/room/<room_key>` へ接続する（ブロッキング）。
    pub fn connect(server_url: &str, room_key: &str) -> Result<Self, WsError> {
        let url = format!("{}/room/{}", server_url, urlencode(room_key));
        let (host, port) = parse_host_port(&url).ok_or(WsError::BadUrl)?;

        let tcp = TcpStream::connect((host.as_str(), port))?;
        let tcp_for_timeout = tcp.try_clone()?;

        // ハンドシェイク自体はタイムアウトを設けず完了させる（往復一回で軽い）。
        let ws = match tungstenite::client_tls(url.as_str(), tcp) {
            Ok((ws, _response)) => ws,
            Err(HandshakeError::Failure(WsProtoError::Http(resp)))
                if resp.status().as_u16() == 403 =>
            {
                return Err(WsError::RoomFull);
            }
            Err(e) => return Err(WsError::Handshake(format!("{:?}", e))),
        };

        // 対局ループ用に読み取りタイムアウトを設定（ハンドシェイク後）。
        tcp_for_timeout.set_read_timeout(Some(READ_TIMEOUT))?;

        let shared = Arc::new(Mutex::new(ws));
        let (tx, rx) = mpsc::channel();
        let reader_handle = Arc::clone(&shared);
        thread::spawn(move || reader_loop(reader_handle, tx));

        Ok(Self {
            ws: shared,
            events: rx,
        })
    }

    /// 対局チャネルのメッセージを送る（公開 API は TCP 殻と共通）。
    pub fn send(&mut self, msg: &WireMessage) -> io::Result<()> {
        self.send_text(msg.to_json())
    }

    /// DO 制御メッセージ（`request_reset` 等）を素の JSON テキストで送る（WS 固有）。
    /// 実 DO 検証（TUI↔TUI・切断/再接続を含む）で request_reset を送らずに
    /// 再開できることを確認済み。online.rs のクラウド再接続は現在これを使わない。
    #[allow(dead_code)]
    pub fn send_raw(&mut self, json: &str) -> io::Result<()> {
        self.send_text(json.to_string())
    }

    fn send_text(&mut self, text: String) -> io::Result<()> {
        let mut ws = self.ws.lock().unwrap();
        ws.send(Message::Text(text.into()))
            .map_err(|e| io::Error::other(e.to_string()))
    }
}

fn reader_loop(ws: Arc<Mutex<Sock>>, tx: Sender<NetEvent>) {
    loop {
        let result = {
            // ロックは read() 呼び出しの間だけ持つ。読み取りタイムアウト（200ms）で
            // 定期的に手放すため、送信側が書き込みで飢餓状態になることはない。
            let mut sock = ws.lock().unwrap();
            sock.read()
        };
        match result {
            Ok(Message::Text(s)) => {
                if let Some(ev) = classify(s.as_str()) {
                    if tx.send(ev).is_err() {
                        return;
                    }
                }
                // 未知の DO メッセージ（spectate_*/record_* 等）は無視して継続する
                // （online.js が game channel の前に捌くのと同じ精神）。
            }
            Ok(Message::Ping(payload)) => {
                let mut sock = ws.lock().unwrap();
                let _ = sock.send(Message::Pong(payload));
            }
            Ok(Message::Close(_)) => {
                let _ = tx.send(NetEvent::Disconnected);
                return;
            }
            Ok(_) => {
                // Binary/Pong/Frame は対局チャネル・DO システムのいずれでもない。
            }
            Err(WsProtoError::Io(e))
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                // 読み取りタイムアウト。切断ではない——ロックを手放して再試行するだけ。
            }
            Err(_) => {
                let _ = tx.send(NetEvent::Disconnected);
                return;
            }
        }
    }
}

/// DO システムメッセージか対局チャネルの `WireMessage` かを分類する。
/// いずれでもない未知の type（spectate_*/record_*/archived 等）は `None`（無視）。
fn classify(s: &str) -> Option<NetEvent> {
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    match v.get("type").and_then(|t| t.as_str())? {
        "peer_joined" | "room_ready" => {
            let side = if v.get("your_side").and_then(|s| s.as_str()) == Some("sente") {
                Side::Sente
            } else {
                Side::Gote
            };
            Some(NetEvent::System(DoSystemMsg::SideAssigned { side }))
        }
        "room_full" => Some(NetEvent::System(DoSystemMsg::RoomFull)),
        "peer_disconnected" => Some(NetEvent::System(DoSystemMsg::PeerDisconnected)),
        "peer_reconnected" => Some(NetEvent::System(DoSystemMsg::PeerReconnected)),
        "you_reconnected" => Some(NetEvent::System(DoSystemMsg::YouReconnected)),
        _ => WireMessage::from_json(s).ok().map(NetEvent::Message),
    }
}

/// `wss://host[:port]/path...` からホストとポートを取り出す（wss の既定は 443）。
fn parse_host_port(url: &str) -> Option<(String, u16)> {
    let rest = url.split_once("://")?.1;
    let host_part = rest.split('/').next()?;
    match host_part.rsplit_once(':') {
        Some((h, p)) => Some((h.to_string(), p.parse().ok()?)),
        None => Some((host_part.to_string(), 443)),
    }
}

/// 部屋キーの最小限のパーセントエンコード（online.js の `encodeURIComponent` に相当）。
fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
