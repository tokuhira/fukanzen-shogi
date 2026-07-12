/// 通信秘匿対戦モード
///
/// commit-reveal-ack プロトコルを `ClientSession`（protocol クレート）に委譲しつつ、
/// I/O を `Transport`（TCP の LAN 殻／WS のクラウド殻）に委譲する。
/// ゲームロジックは `App` を再利用する。
use std::io;
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use engine::board::Position;
use engine::kifu::Kifu;
use engine::movegen::legal_actions;
use engine::types::{Action, Side};
use notation::ja_notation;
use protocol::{
    board_hash, ClientSession, Nonce, RecoverySession, SecretHash, SessionError, SessionEvent,
    WireMessage,
};

use crate::app::{App, OnlineProtocolPhase, OnlineStatus, Phase};
use crate::input;
use crate::net::{Connection, DoSystemMsg, NetEvent};
use crate::net_ws::{self, WsConnection};
use crate::ui;

// ─── 設定 ───────────────────────────────────────────────────────────────────

/// 再接続バックグラウンドスレッドからの結果
enum ReconnectEvent {
    Success(Transport),
    Failed(String),
}

#[derive(Clone)]
pub struct OnlineConfig {
    /// LAN（Listen/Connect）専用の初期値。クラウドでは意味を持たず、
    /// DO の `SideAssigned` が確定させる `side` に上書きされる。
    pub local_side: Side,
    pub mode: ConnectMode,
    pub secret: Vec<u8>,
}

#[derive(Clone)]
pub enum ConnectMode {
    Listen(u16),
    Connect(String),
    /// クラウド参加。部屋キーで DO（`net_ws::CLOUD_SERVER_URL`）の部屋へ入る。
    /// side は選択できない——DO の `peer_joined`/`room_ready` が告げる。
    Cloud {
        room_key: String,
    },
}

// ─── トランスポート抽象（TCP の LAN 殻・WS のクラウド殻を共通に扱う） ──────────

enum Transport {
    Tcp(Connection),
    Ws(WsConnection),
}

impl Transport {
    fn send(&mut self, msg: &WireMessage) -> io::Result<()> {
        match self {
            Transport::Tcp(c) => c.send(msg),
            Transport::Ws(w) => w.send(msg),
        }
    }

    /// 対局チャネル外の制御メッセージ（観戦配信 `spectate_*` 等）を送る。
    /// LAN に観戦者はいないので Tcp は no-op。
    fn send_control(&mut self, json: &str) -> io::Result<()> {
        match self {
            Transport::Tcp(_) => Ok(()),
            Transport::Ws(w) => w.send_raw(json),
        }
    }

    fn events(&self) -> &std::sync::mpsc::Receiver<NetEvent> {
        match self {
            Transport::Tcp(c) => &c.events,
            Transport::Ws(w) => &w.events,
        }
    }
}

// ─── プロトコルフェーズ ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnlinePhase {
    WaitingMyMove,
    WaitingPeerCommit,
    WaitingPeerReveal,
    WaitingPeerAck,
    Disconnected,
    Aborted(String),
}

// ─── メインループ ────────────────────────────────────────────────────────────

/// 接続→対局→終了 までを担う。
pub fn run_online(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: OnlineConfig,
) -> io::Result<()> {
    // ── 接続（side の確定を含む） ────────────────────────────────────────────
    terminal.draw(|f| {
        let area = f.area();
        let para = ratatui::widgets::Paragraph::new(match &config.mode {
            ConnectMode::Listen(port) => format!("ポート {} で接続待ち中...", port),
            ConnectMode::Connect(addr) => format!("{} へ接続中...", addr),
            ConnectMode::Cloud { room_key } => format!("部屋 {} へ接続中...", room_key),
        });
        f.render_widget(para, area);
    })?;

    let (mut transport, side): (Transport, Side) = match &config.mode {
        ConnectMode::Listen(port) => (
            Transport::Tcp(Connection::listen(*port)?),
            config.local_side,
        ),
        ConnectMode::Connect(addr) => (
            Transport::Tcp(Connection::connect(addr)?),
            config.local_side,
        ),
        ConnectMode::Cloud { room_key } => {
            let ws = match WsConnection::connect(net_ws::CLOUD_SERVER_URL, room_key) {
                Ok(ws) => ws,
                Err(e) => {
                    show_error_and_wait_for_quit(terminal, &format!("クラウド接続失敗: {}", e))?;
                    return Ok(());
                }
            };
            let transport = Transport::Ws(ws);
            match wait_for_side_assigned(&transport) {
                Ok(side) => (transport, side),
                Err(msg) => {
                    show_error_and_wait_for_quit(terminal, &msg)?;
                    return Ok(());
                }
            }
        }
    };

    // 観戦配信（延長 4b）: クラウド先手のときだけ spectate_meta/turn/result を送る。
    // LAN に観戦者はいない・後手は web と同じく配信を担わない（淀川 §2 の最小方針）。
    let broadcasting = matches!(config.mode, ConnectMode::Cloud { .. }) && side == Side::Sente;

    // ── ハンドシェイク（hello 集約。版交渉は feed(Hello) の中）───────────────
    let mut session = ClientSession::new(side, &config.secret);
    transport.send(&session.hello_msg())?;
    let version_err: Option<String> = wait_and_feed_hello(&mut transport, &mut session).err();

    // ── 対局準備 ────────────────────────────────────────────────────────────
    let mut app = App::new();
    // 後手側はカーソルを段 1 から開始
    if side == Side::Gote {
        app.phase = Phase::GoteInput;
        app.cursor_rank = 1;
        app.cursor_file = 5;
    }

    let mut online_phase = if let Some(ref msg) = version_err {
        app.message = format!("{} — [q] でポータルへ戻る", msg);
        OnlinePhase::Aborted(msg.clone())
    } else {
        app.message = format!(
            "{}接続完了 — 着手を入力してください",
            if side == Side::Sente {
                "先手: "
            } else {
                "後手: "
            }
        );
        OnlinePhase::WaitingMyMove
    };
    let mut kifu = Kifu::new(Position::initial());

    // spectate_meta（握手完了直後・一度）。version の形は web の version_tuple() に揃える。
    if broadcasting {
        let ver = format!(
            r#"{{"rule":"{}.{}","protocol":{},"app":"{}"}}"#,
            protocol::MY_VERSION.rule.0,
            protocol::MY_VERSION.rule.1,
            protocol::MY_VERSION.protocol,
            env!("CARGO_PKG_VERSION")
        );
        let initial_sfen = engine::serialize::position_to_sfen(&Position::initial());
        let _ = transport.send_control(&format!(
            r#"{{"type":"spectate_meta","version":{},"initial_sfen":"{}"}}"#,
            ver, initial_sfen
        ));
    }

    // 再接続バックグラウンドスレッドからの通知チャネル
    let mut reconnect_rx: Option<std::sync::mpsc::Receiver<ReconnectEvent>> = None;
    // 切断時点で着手が確定済みだったか（再接続後のロールバック通知に使う）
    let mut move_rolled_back = false;

    // 初期状態を online_status に反映（版交渉失敗時は切断扱い）
    sync_online_status(&mut app, &online_phase, side, version_err.is_none());

    loop {
        // ── 描画（online_status は sync_online_status で常に最新） ────────
        terminal.draw(|f| ui::draw(f, &mut app))?;

        // ── 再接続結果の受け取り ─────────────────────────────────────────
        if let Some(ref rx) = reconnect_rx {
            if let Ok(event) = rx.try_recv() {
                reconnect_rx = None;
                match event {
                    ReconnectEvent::Success(new_transport) => {
                        let is_cloud = matches!(new_transport, Transport::Ws(_));
                        transport = new_transport;
                        if is_cloud {
                            // クラウド: DO の you_reconnected を待ってから reconnect_msg を
                            // 送る（§5.4）。ここではソケットの差し替えのみ。
                            app.message =
                                "再接続しました — 相手との再開を待っています...".to_string();
                            sync_online_status(&mut app, &online_phase, side, true);
                        } else {
                            // LAN: 第三段どおり即座に reconnect_msg を送り再開する。
                            let bh = board_hash(&kifu.current());
                            let _ = transport.send(&session.reconnect_msg(bh));
                            resume_after_reconnect(
                                &mut app,
                                side,
                                &mut online_phase,
                                &mut move_rolled_back,
                            );
                        }
                    }
                    ReconnectEvent::Failed(reason) => {
                        online_phase = OnlinePhase::Aborted(format!("再接続失敗: {}", reason));
                        app.message = format!("再接続失敗: {} [q]終了", reason);
                        sync_online_status(&mut app, &online_phase, side, false);
                    }
                }
            }
        }

        if let OnlinePhase::Aborted(_reason) = &online_phase {
            if event::poll(Duration::from_millis(200))? {
                match event::read()? {
                    Event::Key(k) => {
                        use crossterm::event::{KeyCode, KeyEventKind};
                        if k.kind != KeyEventKind::Release
                            && (k.code == KeyCode::Char('q') || k.code == KeyCode::Char('Q'))
                        {
                            break;
                        }
                    }
                    Event::Mouse(m) if input::handle_mouse(m, &mut app) => {
                        break;
                    }
                    _ => {}
                }
            }
            continue;
        }

        // ── ゲームオーバー ────────────────────────────────────────────────
        if let Phase::GameOver(_) = &app.phase {
            if event::poll(Duration::from_millis(200))? {
                match event::read()? {
                    Event::Key(k) => {
                        use crossterm::event::{KeyCode, KeyEventKind};
                        if k.kind != KeyEventKind::Release
                            && (k.code == KeyCode::Char('q') || k.code == KeyCode::Char('Q'))
                        {
                            break;
                        }
                    }
                    Event::Mouse(m) if input::handle_mouse(m, &mut app) => {
                        break;
                    }
                    _ => {}
                }
            }
            continue;
        }

        // ── ネットイベント処理 ────────────────────────────────────────────
        while let Ok(ev) = transport.events().try_recv() {
            match ev {
                NetEvent::Disconnected => {
                    if online_phase != OnlinePhase::Disconnected {
                        // 切断時点で着手が確定済みかを記録（再接続後のロールバック通知用）
                        let my_action = match side {
                            Side::Sente => app.sente_action,
                            Side::Gote => app.gote_action,
                        };
                        move_rolled_back = my_action.is_some();

                        // 初回切断時のみスレッド起動（二重起動防止）
                        online_phase = OnlinePhase::Disconnected;
                        session.abort_turn();
                        app.message = "接続が切断されました — 再接続中...".to_string();
                        sync_online_status(&mut app, &online_phase, side, false);

                        // バックグラウンドで再接続（TUI はブロックしない）。
                        // ソケット/WS 再確立のみ担う——Reconnect 交換はメインループが
                        // 永続 session で駆動する（R1）。
                        let mode = config.mode.clone();
                        let (tx, rx) = std::sync::mpsc::channel::<ReconnectEvent>();
                        reconnect_rx = Some(rx);
                        std::thread::spawn(move || {
                            let result: Result<Transport, String> = match &mode {
                                ConnectMode::Listen(_) | ConnectMode::Connect(_) => {
                                    reconnect_socket_only(&mode)
                                        .map(Transport::Tcp)
                                        .map_err(|e| e.to_string())
                                }
                                ConnectMode::Cloud { room_key } => reconnect_ws_only(room_key)
                                    .map(Transport::Ws)
                                    .map_err(|e| e.to_string()),
                            };
                            let ev = match result {
                                Ok(t) => ReconnectEvent::Success(t),
                                Err(e) => ReconnectEvent::Failed(e),
                            };
                            let _ = tx.send(ev);
                        });
                    }
                }
                NetEvent::System(sys) => {
                    match sys {
                        DoSystemMsg::PeerDisconnected => {
                            app.message = "相手が切断しました。再接続を待っています…".to_string();
                        }
                        DoSystemMsg::YouReconnected => {
                            // 自分が再接続した（WS 再確立後、DO が告げる）。
                            // 現局面で reconnect を送り、以降は feed 分岐で再開する。
                            let bh = board_hash(&kifu.current());
                            let _ = transport.send(&session.reconnect_msg(bh));
                            resume_after_reconnect(
                                &mut app,
                                side,
                                &mut online_phase,
                                &mut move_rolled_back,
                            );
                        }
                        DoSystemMsg::PeerReconnected => {
                            app.message = "相手が再接続しました。".to_string();
                        }
                        DoSystemMsg::RoomFull => {
                            online_phase = OnlinePhase::Aborted("この部屋は満室です".to_string());
                        }
                        DoSystemMsg::SideAssigned { .. } => {
                            // ハンドシェイク後は無視（side は既に確定済み）
                        }
                    }
                    sync_online_status(&mut app, &online_phase, side, true);
                }
                NetEvent::Message(wire) => {
                    match session.feed(wire) {
                        Ok(SessionEvent::PeerCommitted { both_committed }) => {
                            if both_committed {
                                match session.reveal_msg() {
                                    Ok(reveal) => {
                                        transport.send(&reveal)?;
                                        online_phase = OnlinePhase::WaitingPeerReveal;
                                        app.message =
                                            "Reveal 送信済み — 相手の Reveal 待ち...".to_string();
                                    }
                                    Err(e) => abort(
                                        &mut online_phase,
                                        &mut transport,
                                        format!("reveal 生成エラー: {:?}", e),
                                    ),
                                }
                            } else {
                                app.message =
                                    "相手のコミット受信済み — 自分の着手を確定してください"
                                        .to_string();
                            }
                        }
                        Ok(SessionEvent::PeerCommitBuffered) => {
                            app.message =
                                "相手のコミット受信済み — 着手を入力してください".to_string();
                        }
                        Ok(SessionEvent::PeerRevealed { both_revealed }) => {
                            if both_revealed {
                                match session.ack_msg() {
                                    Ok(ack) => {
                                        transport.send(&ack)?;
                                        online_phase = OnlinePhase::WaitingPeerAck;
                                        app.message =
                                            "Ack 送信済み — 相手の Ack 待ち...".to_string();
                                    }
                                    Err(e) => abort(
                                        &mut online_phase,
                                        &mut transport,
                                        format!("ack エラー: {:?}", e),
                                    ),
                                }
                            }
                        }
                        Ok(SessionEvent::TurnComplete { sente, gote }) => {
                            // (a) この手を観戦者へ（両者公開後なので秘匿を破らない）。
                            // 投了手も送る（web と同順）。
                            if broadcasting {
                                let _ = transport.send_control(&format!(
                                    r#"{{"type":"spectate_turn","s":"{}","g":"{}"}}"#,
                                    sente.to_usi(),
                                    gote.to_usi()
                                ));
                            }
                            // (b) 従来どおり解決（投了・盤面終局を game_result 経由で GameOver に）。
                            resolve_completed_turn(
                                sente,
                                gote,
                                &mut app,
                                &mut kifu,
                                &mut online_phase,
                                &mut transport,
                                side,
                            );
                            // (c) 終局したら結果を観戦者へ。単一正本 game_result を直接呼ぶ
                            // （手組みの結果表は作らない——max_turns・投了も正しく出る）。
                            if broadcasting {
                                if let Phase::GameOver(_) = app.phase {
                                    if let Some((kind, outcome)) = protocol::game_result(&kifu) {
                                        let _ = transport.send_control(&format!(
                                            r#"{{"type":"spectate_result","kind":"{}","outcome":"{}"}}"#,
                                            kind.to_str(),
                                            outcome.to_str()
                                        ));
                                    }
                                }
                            }
                        }
                        Ok(SessionEvent::PeerAborted { reason }) => {
                            abort_to(&mut online_phase, format!("相手がアボート: {}", reason));
                        }
                        Ok(SessionEvent::PeerReconnectRequest { board_hash: bh }) => {
                            let recovery = RecoverySession::new(
                                kifu.clone(),
                                session.peer_auth_hash().unwrap_or(SecretHash([0u8; 32])),
                            );
                            if recovery.find_resume_point(bh).is_some() {
                                let _ = transport.send(&session.reconnect_ack_msg(bh));
                            } else {
                                abort(
                                    &mut online_phase,
                                    &mut transport,
                                    "hash_mismatch".to_string(),
                                );
                            }
                        }
                        Ok(SessionEvent::ReconnectAck { resume_hash }) => {
                            let recovery = RecoverySession::new(
                                kifu.clone(),
                                session.peer_auth_hash().unwrap_or(SecretHash([0u8; 32])),
                            );
                            if recovery.find_resume_point(resume_hash).is_none() {
                                abort_to(
                                    &mut online_phase,
                                    "再接続: 再開局面が見つかりません".to_string(),
                                );
                            }
                        }
                        Err(SessionError::IdentityMismatch) => {
                            abort(
                                &mut online_phase,
                                &mut transport,
                                "auth_mismatch".to_string(),
                            );
                        }
                        Err(e) => {
                            abort(
                                &mut online_phase,
                                &mut transport,
                                format!("プロトコルエラー: {:?}", e),
                            );
                        }
                        Ok(SessionEvent::HandshakeDone { .. }) | Ok(SessionEvent::PeerAcked) => {
                            // ループ中は無視/待機
                        }
                    }
                    sync_online_status(&mut app, &online_phase, side, true);
                }
            }
        }

        // ── 着手入力フェーズのみキー受付 ─────────────────────────────────
        if online_phase != OnlinePhase::WaitingMyMove {
            // プロトコル待機中はキー入力を処理しない（q は抜け）
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(k) = event::read()? {
                    use crossterm::event::{KeyCode, KeyEventKind};
                    if k.kind != KeyEventKind::Release
                        && (k.code == KeyCode::Char('q') || k.code == KeyCode::Char('Q'))
                    {
                        break;
                    }
                }
            }
            continue;
        }

        // ── キー/マウス入力 ───────────────────────────────────────────────
        if event::poll(Duration::from_millis(50))? {
            let ev = event::read()?;
            match ev {
                Event::Key(k) => {
                    use crossterm::event::{KeyCode, KeyEventKind};
                    if k.kind == KeyEventKind::Release {
                        // Release は無視（Windows CMD チャタリング対策）
                    } else if k.code == KeyCode::Char('q') || k.code == KeyCode::Char('Q') {
                        break;
                    } else if k.code == KeyCode::Char('r') || k.code == KeyCode::Char('R') {
                        // オンライン投了: 即終局ではなく commit-reveal プロトコル経由で投了
                        match side {
                            Side::Sente => app.sente_action = Some(Action::Resign),
                            Side::Gote => app.gote_action = Some(Action::Resign),
                        }
                        app.message = "投了申告 — 相手の確定を待っています...".to_string();
                    } else {
                        input::handle_key(k, &mut app);
                    }
                }
                Event::Mouse(m) => {
                    if input::handle_mouse(m, &mut app) {
                        return Ok(());
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        // ── 着手が確定したか検出 ─────────────────────────────────────────
        let my_action = match side {
            Side::Sente => app.sente_action,
            Side::Gote => app.gote_action,
        };

        if let Some(action) = my_action {
            // UI を "待機中" に固定
            app.phase = Phase::ResolveReady;
            let pos = kifu.current();
            let la = legal_actions(&pos, side);
            let notation = ja_notation(&action, side, &la, &pos);
            app.message = format!("着手確定: {}", notation);

            // commit-reveal セッション開始
            let pos = kifu.current();
            let pos_hash = board_hash(&pos);
            let nonce = random_nonce();
            match session.commit(pos_hash, action, nonce) {
                Ok(commit_msg) => {
                    transport.send(&commit_msg)?;
                    online_phase = OnlinePhase::WaitingPeerCommit;
                    sync_online_status(&mut app, &online_phase, side, true);

                    // 先着していた peer commit があれば commit() の中で適用済み
                    // → 両者揃っていれば即 reveal
                    if session.both_committed() {
                        let reveal = session
                            .reveal_msg()
                            .expect("both_committed 済みなら reveal 可");
                        transport.send(&reveal)?;
                        online_phase = OnlinePhase::WaitingPeerReveal;
                        sync_online_status(&mut app, &online_phase, side, true);
                    }
                }
                Err(e) => {
                    let reason = format!("commit エラー: {:?}", e);
                    online_phase = OnlinePhase::Aborted(reason.clone());
                    let _ = transport.send(&WireMessage::Abort { reason });
                }
            }
        }
    }

    Ok(())
}

// ─── ターン完了の反映（投了判定を含む・出典: 旧 handle_net_message の Ack 分岐）──

fn resolve_completed_turn(
    sente_action: Action,
    gote_action: Action,
    app: &mut App,
    kifu: &mut Kifu,
    online_phase: &mut OnlinePhase,
    transport: &mut Transport,
    side: Side,
) {
    // 投了判定（ルール 5.3 / 5.4）: resolve を通さず、投了組手を積んで単一正本 game_result へ委ねる。
    let s_resign = sente_action.is_resign();
    let g_resign = gote_action.is_resign();
    if s_resign || g_resign {
        use engine::types::Ply;
        kifu.push(Ply {
            sente: sente_action,
            gote: gote_action,
        });
        if let Some((kind, outcome)) = protocol::game_result(kifu) {
            app.phase = Phase::GameOver(crate::app::game_over_from_result(kind, outcome));
        }
        return;
    }

    // resolve() は両着手が現局面で既に合法であることを前提とする（engine 側の契約）。
    // 相手の reveal はここまで拘束性・盤面ハッシュしか検証されておらず合法性は未検証
    // なので、resolve へ渡す前にここで確認する——さもないと空マスからの移動や
    // 成れない駒の成り宣言のような非合法な reveal で resolve() がパニックする
    // （不正な相手・改竄されたクライアントからの攻撃面）。
    if !turn_actions_are_legal(&app.current_pos(), sente_action, gote_action) {
        abort(
            online_phase,
            transport,
            "相手から非合法な着手を受信しました".to_string(),
        );
        return;
    }

    // 通常の着手: kifu に記録して resolve
    app.sente_action = Some(sente_action);
    app.gote_action = Some(gote_action);
    app.resolve_turn();

    use engine::types::Ply;
    kifu.push(Ply {
        sente: sente_action,
        gote: gote_action,
    });

    if !matches!(app.phase, Phase::GameOver(_)) {
        // 次ターンへ
        *online_phase = OnlinePhase::WaitingMyMove;
        if side == Side::Gote {
            // resolve_turn は常に SenteInput に戻すので上書き
            app.phase = Phase::GoteInput;
            app.cursor_rank = 1;
        }
        app.message = "次の着手を入力してください".to_string();
    }
}

/// 再接続後の「捨てて指し直し」への復帰（出典: 第三段の `ReconnectEvent::Success`）。
/// LAN は再接続直後に、クラウドは `YouReconnected` を受けてから呼ぶ。
fn resume_after_reconnect(
    app: &mut App,
    side: Side,
    online_phase: &mut OnlinePhase,
    move_rolled_back: &mut bool,
) {
    *online_phase = OnlinePhase::WaitingMyMove;
    app.sente_action = None;
    app.gote_action = None;
    match side {
        Side::Sente => {
            app.phase = Phase::SenteInput;
        }
        Side::Gote => {
            app.phase = Phase::GoteInput;
            app.cursor_rank = 1;
        }
    }
    app.message = if *move_rolled_back {
        *move_rolled_back = false;
        "着手をキャンセルしました — 再度入力してください".to_string()
    } else {
        "着手を入力してください".to_string()
    };
    sync_online_status(app, online_phase, side, true);
    notify_reconnect(app, 4);
}

/// 両陣営の着手が現局面で合法かを検証する（`resolve()` へ渡す前の安全弁）。
/// `resolve()` は両着手が既に合法であることを前提とする契約なので、相手の reveal
/// （拘束性・盤面ハッシュしか検証されていない）をそのまま渡すとパニックしうる。
fn turn_actions_are_legal(pos: &Position, sente: Action, gote: Action) -> bool {
    legal_actions(pos, Side::Sente).contains(&sente)
        && legal_actions(pos, Side::Gote).contains(&gote)
}

// ─── アボートの小さなヘルパ ─────────────────────────────────────────────────

/// 自分の判定でアボートし、相手にも通知する（Abort を送る）。
fn abort(online_phase: &mut OnlinePhase, transport: &mut Transport, reason: String) {
    *online_phase = OnlinePhase::Aborted(reason.clone());
    let _ = transport.send(&WireMessage::Abort { reason });
}

/// 相手が既にアボート済み、または通知不要な場合の局所反映のみ。
fn abort_to(online_phase: &mut OnlinePhase, reason: String) {
    *online_phase = OnlinePhase::Aborted(reason);
}

// ─── ハンドシェイク補助 ─────────────────────────────────────────────────────

/// peer の Hello を待って feed する。成功で peer_side、失敗で整形済みエラー文字列。
/// クラウドでは対局チャネル以外の `System` イベントも届き得るため、Hello 以外の
/// メッセージは黙って読み飛ばして待ち続ける。
fn wait_and_feed_hello(
    transport: &mut Transport,
    session: &mut ClientSession,
) -> Result<Side, String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(
                "版交渉: 相手が応答しませんでした（版交渉未対応の版かもしれません）".to_string(),
            );
        }
        match transport.events().try_recv() {
            Ok(NetEvent::Message(wire @ WireMessage::Hello { .. })) => {
                return match session.feed(wire) {
                    Ok(SessionEvent::HandshakeDone { peer_side }) => Ok(peer_side),
                    Err(SessionError::VersionMismatch(o)) => Err(format_version_mismatch(&o)),
                    Err(e) => Err(format!("ハンドシェイク失敗: {:?}", e)),
                    Ok(_) => Err("ハンドシェイク: 予期しない応答".to_string()),
                };
            }
            Ok(NetEvent::Message(_)) => return Err("ハンドシェイク: hello 以外を受信".to_string()),
            Ok(NetEvent::System(_)) => {
                // クラウドの部屋メッセージが紛れても無視して hello を待ち続ける。
            }
            Ok(NetEvent::Disconnected) => return Err("ハンドシェイク中に切断".to_string()),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// クラウド接続直後、DO が `SideAssigned` を告げるのを待つ。
fn wait_for_side_assigned(transport: &Transport) -> Result<Side, String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if std::time::Instant::now() > deadline {
            return Err("相手を待っています…がタイムアウトしました".to_string());
        }
        match transport.events().try_recv() {
            Ok(NetEvent::System(DoSystemMsg::SideAssigned { side })) => return Ok(side),
            Ok(NetEvent::System(DoSystemMsg::RoomFull)) => {
                return Err("この部屋は満室です".to_string())
            }
            Ok(NetEvent::Disconnected) => return Err("接続が切断されました".to_string()),
            Ok(_) => {
                // 他の System/Message は無視して待ち続ける。
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// 接続前・handshake 前の致命的な失敗を表示し、[q] でポータルへ戻る。
fn show_error_and_wait_for_quit(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    message: &str,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| {
            let area = f.area();
            let para =
                ratatui::widgets::Paragraph::new(format!("{} — [q] でポータルへ戻る", message));
            f.render_widget(para, area);
        })?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(k) = event::read()? {
                use crossterm::event::{KeyCode, KeyEventKind};
                if k.kind != KeyEventKind::Release
                    && (k.code == KeyCode::Char('q') || k.code == KeyCode::Char('Q'))
                {
                    return Ok(());
                }
            }
        }
    }
}

// ─── 再接続（トランスポート再確立のみ。Reconnect 交換はメインループが担う・R1）─

/// LAN（TCP）のソケット再確立のみ。呼び出し元は `ConnectMode::Listen`/`Connect` のみで呼ぶ。
fn reconnect_socket_only(mode: &ConnectMode) -> io::Result<Connection> {
    match mode {
        ConnectMode::Listen(port) => Connection::listen(*port),
        ConnectMode::Connect(addr) => {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            loop {
                match Connection::connect(addr) {
                    Ok(c) => return Ok(c),
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        ConnectMode::Cloud { .. } => unreachable!("cloud は reconnect_ws_only を使う"),
    }
}

/// クラウド（WS）の再確立のみ。DO は同じ部屋キーへの再入室を
/// 既存の 2 人部屋への再接続として扱い、`you_reconnected`/`peer_reconnected` を送る。
fn reconnect_ws_only(room_key: &str) -> Result<WsConnection, net_ws::WsError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        match WsConnection::connect(net_ws::CLOUD_SERVER_URL, room_key) {
            Ok(ws) => return Ok(ws),
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(e) => return Err(e),
        }
    }
}

// ─── バージョン不一致の整形 ──────────────────────────────────────────────────

fn format_version_mismatch(outcome: &protocol::NegotiationOutcome) -> String {
    use protocol::NegotiationOutcome;
    match outcome {
        NegotiationOutcome::Incompatible {
            mine,
            theirs,
            rule_mismatch,
            protocol_mismatch,
        } => {
            let mut parts = Vec::new();
            if *rule_mismatch {
                parts.push(format!(
                    "ルール版: 自分 {}.{} ≠ 相手 {}.{}",
                    mine.rule.0, mine.rule.1, theirs.rule.0, theirs.rule.1
                ));
            }
            if *protocol_mismatch {
                parts.push(format!(
                    "プロトコル版: 自分 {} ≠ 相手 {}",
                    mine.protocol, theirs.protocol
                ));
            }
            format!("版が異なるため対戦できません。{}", parts.join("、"))
        }
        NegotiationOutcome::InvalidResponse => {
            "版交渉: 相手の応答が不正です。互換性のない版の可能性があります。".to_string()
        }
        NegotiationOutcome::Timeout => {
            "版交渉: 相手が応答しませんでした。版交渉に対応していない版（v0.5.0 以前）の可能性があります。".to_string()
        }
    }
}

// ─── ユーティリティ ─────────────────────────────────────────────────────────

fn random_nonce() -> Nonce {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    Nonce(bytes)
}

/// `OnlinePhase` の変化を `app.online_status` へ反映する。
/// 既存の `reconnect_notice_until` は引き継ぐ（期限切れ確認は ui 側で行う）。
fn sync_online_status(app: &mut App, phase: &OnlinePhase, local_side: Side, connected: bool) {
    let protocol = match phase {
        OnlinePhase::WaitingMyMove => OnlineProtocolPhase::MyTurn,
        OnlinePhase::WaitingPeerCommit => OnlineProtocolPhase::PeerCommitPending,
        OnlinePhase::WaitingPeerReveal => OnlineProtocolPhase::PeerRevealPending,
        OnlinePhase::WaitingPeerAck => OnlineProtocolPhase::PeerAckPending,
        OnlinePhase::Disconnected => OnlineProtocolPhase::Disconnected,
        OnlinePhase::Aborted(r) => OnlineProtocolPhase::Aborted(r.clone()),
    };
    let peer_revealed = match local_side {
        Side::Sente => app.gote_action.is_some(),
        Side::Gote => app.sente_action.is_some(),
    };
    let reconnect_notice_until = app
        .online_status
        .as_ref()
        .and_then(|s| s.reconnect_notice_until);
    app.online_status = Some(OnlineStatus {
        local_side,
        protocol,
        connected,
        peer_revealed,
        reconnect_notice_until,
    });
}

/// 再接続成功時に呼ぶ。`duration` 秒間だけ通知を表示する。
fn notify_reconnect(app: &mut App, duration_secs: u64) {
    if let Some(ref mut os) = app.online_status {
        os.reconnect_notice_until =
            Some(std::time::Instant::now() + Duration::from_secs(duration_secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合法な双方の着手は通過する。
    #[test]
    fn legal_actions_pass() {
        let pos = Position::initial();
        let sente = Action::from_usi("7g7f").unwrap();
        let gote = Action::from_usi("3c3d").unwrap();
        assert!(turn_actions_are_legal(&pos, sente, gote));
    }

    /// 悪意ある相手が「駒のないマスからの移動」を reveal しても resolve() へは進まない。
    #[test]
    fn empty_from_square_rejected() {
        let pos = Position::initial();
        let malicious = Action::from_usi("5e5f").unwrap(); // 5e は初期局面で空
        let honest = Action::from_usi("3c3d").unwrap();
        assert!(!turn_actions_are_legal(&pos, honest, malicious));
    }

    /// 悪意ある相手が「玉に成る」ような非合法な reveal を送っても弾かれる。
    #[test]
    fn illegal_promote_rejected() {
        let pos = Position::initial();
        let malicious = Action::from_usi("5i5h+").unwrap(); // 玉は成れない
        let honest = Action::from_usi("3c3d").unwrap();
        assert!(!turn_actions_are_legal(&pos, malicious, honest));
    }

    /// 相手の駒を自分の着手であるかのように動かす reveal も弾かれる
    /// （from に自分の駒がなければ legal_actions に含まれない）。
    #[test]
    fn cross_side_piece_rejected() {
        let pos = Position::initial();
        // 後手が "7g7f"（実際は先手の歩の位置）を自分の着手と偽る
        let malicious_gote = Action::from_usi("7g7f").unwrap();
        let honest_sente = Action::from_usi("2g2f").unwrap();
        assert!(!turn_actions_are_legal(&pos, honest_sente, malicious_gote));
    }
}
