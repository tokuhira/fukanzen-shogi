/// WebSocket 通信殻（クラウド参加）。
///
/// `tui/src/net.rs` の TCP 殻と同じ公開 API（`send(&WireMessage)` / `events`）を、
/// 同期 `tungstenite`＋rustls の上で実装する。DO（Durable Object）の部屋
/// （`wss://…/room/<部屋キー>`）へ接続し、対局チャネルの `WireMessage` と
/// DO のシステムメッセージ（side 割り当て・切断・再接続・満室）を
/// `NetEvent` へ分類して surface する。
///
/// tungstenite の `WebSocket<Stream>` は読み書きの状態（ping/pong の自動応答・
/// メッセージ再構築バッファ）を一つの値が抱えるため安全に分割できない。
/// そこで読み書きの両方を**単一の IO スレッドだけ**が所有し、送信は
/// `mpsc::Sender` 経由でそのスレッドへ委譲する（メインスレッドは送信要求を
/// キューへ積むだけで、ネットワーク I/O 自体には触れない）。
///
/// 以前は `Arc<Mutex<WebSocket>>` を reader スレッドとメインスレッドで
/// 共有していたが、reader が読み取りタイムアウト（200ms）ごとに
/// unlock→即 relock を繰り返すループのため、送信側がロックを取り損ね続けて
/// 実質的なロック飢餓（最悪で数十秒の送信遅延）が起きた（実 DO・実機で観測）。
/// IO スレッドを一つに絞ることで、送信はロック待ちを経由せず即座にキューへ
/// 積め、実際の書き込みは次の読み取りタイムアウト周期（最大 `READ_TIMEOUT`）
/// 以内に決定的に行われる——スピンロックではなく、単一スレッドの
/// 定周期ポーリングなので CPU を無駄に消費しない。
use std::io;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
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

/// IO ループのブロッキング読み取りタイムアウト。この周期で読み取りから戻り、
/// 送信キューを確認する——送信要求からの最悪遅延の上限になる。
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
    to_send: Sender<String>,
    pub events: Receiver<NetEvent>,
}

impl WsConnection {
    /// `server_url/room/<room_key>` へ接続する（ブロッキング）。
    pub fn connect(server_url: &str, room_key: &str) -> Result<Self, WsError> {
        let url = format!("{}/room/{}", server_url, urlencode(room_key));
        let (host, port) = parse_host_port(&url).ok_or(WsError::BadUrl)?;

        let tcp = TcpStream::connect((host.as_str(), port))?;
        // commit-reveal-ack は小さなメッセージの往復（ping-pong）なので、Nagle
        // アルゴリズム（既定で有効）が自分の送信を相手の ACK 待ちで足止めしうる。
        // 相手（delayed ACK タイマー）の挙動次第で数百ms〜数秒の余計な遅延になる
        // ——実機で Windows 版のみコミット・リビールがそれぞれ 1-2 秒余計に
        // かかる形で顕在化した。無効化して都度即座に送る。
        tcp.set_nodelay(true)?;
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

        // IO ループ用に読み取りタイムアウトを設定（ハンドシェイク後）。
        tcp_for_timeout.set_read_timeout(Some(READ_TIMEOUT))?;

        let (tx_out, rx_out) = mpsc::channel::<String>();
        let (tx_in, rx_in) = mpsc::channel();
        thread::spawn(move || io_loop(ws, rx_out, tx_in));

        Ok(Self {
            to_send: tx_out,
            events: rx_in,
        })
    }

    /// 対局チャネルのメッセージを送る（公開 API は TCP 殻と共通）。
    /// 実際の書き込みは IO スレッドが行う——ここではキューへ積むだけなので
    /// ネットワーク I/O やロック待ちで一切ブロックしない。
    pub fn send(&mut self, msg: &WireMessage) -> io::Result<()> {
        self.queue(msg.to_json())
    }

    /// DO 制御メッセージ（`spectate_meta`/`spectate_turn`/`spectate_result` 等）を
    /// 素の JSON テキストで送る（WS 固有・対局チャネル外）。TUI 先手のクラウド
    /// 観戦配信（延長 4b）が使う。
    pub fn send_raw(&mut self, json: &str) -> io::Result<()> {
        self.queue(json.to_string())
    }

    fn queue(&mut self, text: String) -> io::Result<()> {
        self.to_send
            .send(text)
            .map_err(|_| io::Error::other("WS IO スレッドが終了しています"))
    }
}

/// 読み書きの両方を単一スレッドで担う。読み取り（最大 `READ_TIMEOUT` でブロック）
/// → 送信キューの排出、を交互に繰り返す。`ws` はこのスレッドだけが所有するため
/// ロックが要らない。
fn io_loop(mut ws: Sock, rx_out: Receiver<String>, tx_in: Sender<NetEvent>) {
    loop {
        match ws.read() {
            Ok(Message::Text(s)) => {
                if let Some(ev) = classify(s.as_str()) {
                    if tx_in.send(ev).is_err() {
                        return;
                    }
                }
                // 未知の DO メッセージ（spectate_*/record_* 等）は無視して継続する
                // （online.js が game channel の前に捌くのと同じ精神）。
            }
            Ok(Message::Close(_)) => {
                let _ = tx_in.send(NetEvent::Disconnected);
                return;
            }
            Ok(_) => {
                // Ping/Pong/Binary/Frame。Ping への Pong 応答は tungstenite が
                // 自動でキューし、次の read/write/flush で送られる
                // （手動で応答しないよう tungstenite 自身が案内している）。
            }
            Err(WsProtoError::Io(e))
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                // 読み取りタイムアウト。切断ではない——送信キューを確認して継続する。
            }
            Err(_) => {
                let _ = tx_in.send(NetEvent::Disconnected);
                return;
            }
        }

        // 送信キューを排出する（非ブロッキング）。書き込み失敗は切断として扱う。
        while let Ok(text) = rx_out.try_recv() {
            if ws.send(Message::Text(text.into())).is_err() {
                let _ = tx_in.send(NetEvent::Disconnected);
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
