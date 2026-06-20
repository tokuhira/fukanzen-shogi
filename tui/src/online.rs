/// 通信秘匿対戦モード
///
/// commit-reveal-ack プロトコルを `TurnSession` に委譲しつつ、
/// TCP I/O を `Connection` に委譲する。
/// ゲームロジックは `App` を再利用する。
use std::io;
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use engine::board::Position;
use engine::kifu::Kifu;
use engine::types::{Action, Side};
use protocol::{
    board_hash, hash_secret, Nonce, RecoverySession, SecretHash, TurnSession,
};

use crate::app::{App, OnlineProtocolPhase, OnlineStatus, Phase};
use crate::input;
use crate::net::{
    self, Connection, NetEvent, NetMessage,
    board_hash_from_hex, board_hash_to_hex,
    commitment_from_hex, commitment_to_hex,
    nonce_from_hex, nonce_to_hex,
};
use crate::ui;

// ─── 設定 ───────────────────────────────────────────────────────────────────

/// 再接続バックグラウンドスレッドからの結果
enum ReconnectEvent {
    Success(Connection),
    Failed(String),
}

#[derive(Clone)]
pub struct OnlineConfig {
    pub local_side: Side,
    pub mode: ConnectMode,
    pub secret: Vec<u8>,
}

#[derive(Clone)]
pub enum ConnectMode {
    Listen(u16),
    Connect(String),
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
    // ── 接続 ───────────────────────────────────────────────────────────────
    terminal.draw(|f| {
        let area = f.area();
        let para = ratatui::widgets::Paragraph::new(match &config.mode {
            ConnectMode::Listen(port) => format!("ポート {} で接続待ち中...", port),
            ConnectMode::Connect(addr) => format!("{} へ接続中...", addr),
        });
        f.render_widget(para, area);
    })?;

    let mut conn = match &config.mode {
        ConnectMode::Listen(port) => Connection::listen(*port)?,
        ConnectMode::Connect(addr) => Connection::connect(addr)?,
    };

    // ── ハンドシェイク ──────────────────────────────────────────────────────
    let secret_hash = hash_secret(&config.secret);
    let my_side_u8 = match config.local_side { Side::Sente => 0u8, Side::Gote => 1u8 };

    conn.send(&NetMessage::GameStart {
        side: my_side_u8,
        secret_hash: net::to_hex(&secret_hash.0),
    })?;

    let peer_secret_hash = wait_game_start(&mut conn)?;

    // ── 対局 ───────────────────────────────────────────────────────────────
    let mut app = App::new();
    // 後手側はカーソルを段 1 から開始
    if config.local_side == Side::Gote {
        app.phase = Phase::GoteInput;
        app.cursor_rank = 1;
        app.cursor_file = 5;
    }
    app.message = format!("{}接続完了 — 着手を入力してください",
        if config.local_side == Side::Sente { "先手: " } else { "後手: " });

    let mut online_phase = OnlinePhase::WaitingMyMove;
    let mut turn_session: Option<TurnSession> = None;
    let mut pending_peer_commit: Option<protocol::Commitment> = None;
    let mut kifu = Kifu::new(Position::initial());
    // 再接続バックグラウンドスレッドからの通知チャネル
    let mut reconnect_rx: Option<std::sync::mpsc::Receiver<ReconnectEvent>> = None;

    // 初期状態を online_status に反映
    sync_online_status(&mut app, &online_phase, config.local_side, true);

    loop {
        // ── 描画（online_status は sync_online_status で常に最新） ────────
        terminal.draw(|f| ui::draw(f, &mut app))?;

        // ── 再接続結果の受け取り ─────────────────────────────────────────
        if let Some(ref rx) = reconnect_rx {
            if let Ok(event) = rx.try_recv() {
                reconnect_rx = None;
                match event {
                    ReconnectEvent::Success(new_conn) => {
                        conn = new_conn;
                        online_phase = OnlinePhase::WaitingMyMove;
                        turn_session = None;
                        pending_peer_commit = None;
                        app.sente_action = None;
                        app.gote_action = None;
                        match config.local_side {
                            Side::Sente => { app.phase = Phase::SenteInput; }
                            Side::Gote  => { app.phase = Phase::GoteInput; app.cursor_rank = 1; }
                        }
                        app.message = "再接続しました — 着手を入力してください".to_string();
                        sync_online_status(&mut app, &online_phase, config.local_side, true);
                    }
                    ReconnectEvent::Failed(reason) => {
                        online_phase = OnlinePhase::Aborted(format!("再接続失敗: {}", reason));
                        app.message = format!("再接続失敗: {} [q]終了", reason);
                        sync_online_status(&mut app, &online_phase, config.local_side, false);
                    }
                }
            }
        }

        if let OnlinePhase::Aborted(_reason) = &online_phase {
            // ゲーム終了 (アボート) — Q で抜ける
            if event::poll(Duration::from_millis(200))? {
                if let Event::Key(k) = event::read()? {
                    use crossterm::event::KeyCode;
                    if k.code == KeyCode::Char('q') || k.code == KeyCode::Char('Q') {
                        break;
                    }
                }
            }
            continue;
        }

        // ── ゲームオーバー ────────────────────────────────────────────────
        if let Phase::GameOver(_) = &app.phase {
            if event::poll(Duration::from_millis(200))? {
                if let Event::Key(k) = event::read()? {
                    use crossterm::event::KeyCode;
                    if k.code == KeyCode::Char('q') || k.code == KeyCode::Char('Q') {
                        break;
                    }
                }
            }
            continue;
        }

        // ── ネットイベント処理 ────────────────────────────────────────────
        while let Ok(ev) = conn.events.try_recv() {
            match ev {
                NetEvent::Disconnected => {
                    if online_phase != OnlinePhase::Disconnected {
                        // 初回切断時のみスレッド起動（二重起動防止）
                        online_phase = OnlinePhase::Disconnected;
                        turn_session = None;
                        pending_peer_commit = None;
                        app.message = "接続が切断されました — 再接続中...".to_string();
                        sync_online_status(&mut app, &online_phase, config.local_side, false);

                        // バックグラウンドで再接続（TUI はブロックしない）
                        let config2 = config.clone();
                        let kifu2 = kifu.clone();
                        let peer_hash2 = peer_secret_hash;
                        let (tx, rx) = std::sync::mpsc::channel::<ReconnectEvent>();
                        reconnect_rx = Some(rx);
                        std::thread::spawn(move || {
                            let result = reconnect(&config2, &kifu2, &peer_hash2);
                            let ev = match result {
                                Ok(conn) => ReconnectEvent::Success(conn),
                                Err(e)   => ReconnectEvent::Failed(e.to_string()),
                            };
                            let _ = tx.send(ev);
                        });
                    }
                }
                NetEvent::Message(msg) => {
                    if let Err(abort_reason) = handle_net_message(
                        msg,
                        &mut online_phase,
                        &mut turn_session,
                        &mut pending_peer_commit,
                        &mut app,
                        &mut conn,
                        &config.local_side,
                        &mut kifu,
                    ) {
                        online_phase = OnlinePhase::Aborted(abort_reason.clone());
                        let _ = conn.send(&NetMessage::Abort { reason: abort_reason });
                    }
                    sync_online_status(&mut app, &online_phase, config.local_side, true);
                }
            }
        }

        // ── 着手入力フェーズのみキー受付 ─────────────────────────────────
        if online_phase != OnlinePhase::WaitingMyMove {
            // プロトコル待機中はキー入力を処理しない（q は抜け）
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(k) = event::read()? {
                    use crossterm::event::KeyCode;
                    if k.code == KeyCode::Char('q') || k.code == KeyCode::Char('Q') {
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
                    use crossterm::event::KeyCode;
                    if k.code == KeyCode::Char('q') || k.code == KeyCode::Char('Q') {
                        break;
                    }
                    input::handle_key(k, &mut app);
                }
                Event::Mouse(m) => {
                    input::handle_mouse(m, &mut app);
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        // ── 着手が確定したか検出 ─────────────────────────────────────────
        let my_action = match config.local_side {
            Side::Sente => app.sente_action,
            Side::Gote  => app.gote_action,
        };

        if let Some(action) = my_action {
            // UI を "待機中" に固定
            app.phase = Phase::ResolveReady;
            app.message = format!("着手確定: {}", action.to_usi());

            // commit-reveal セッション開始
            let pos = kifu.current();
            let pos_hash = board_hash(&pos);
            let nonce = random_nonce();
            let mut session = TurnSession::new(config.local_side, pos_hash);
            let commitment = session.local_commit(action, nonce).expect("first commit");

            // Commit 送信
            conn.send(&NetMessage::Commit {
                commitment: commitment_to_hex(&commitment),
            })?;

            turn_session = Some(session);
            online_phase = OnlinePhase::WaitingPeerCommit;
            sync_online_status(&mut app, &online_phase, config.local_side, true);

            // 自分より先に相手のコミットが届いていた場合は即座に適用
            if let Some(pending) = pending_peer_commit.take() {
                let session = turn_session.as_mut().unwrap();
                if session.receive_peer_commit(pending).is_ok() && session.both_committed() {
                    if let Ok(reveal) = session.local_reveal() {
                        let _ = conn.send(&NetMessage::Reveal {
                            action_usi: reveal.action.to_usi(),
                            nonce: nonce_to_hex(&reveal.nonce),
                            board_hash: board_hash_to_hex(&reveal.board_hash),
                        });
                        online_phase = OnlinePhase::WaitingPeerReveal;
                        sync_online_status(&mut app, &online_phase, config.local_side, true);
                    }
                }
            }
        }
    }

    Ok(())
}

// ─── ネットメッセージハンドラ ────────────────────────────────────────────────

fn handle_net_message(
    msg: NetMessage,
    online_phase: &mut OnlinePhase,
    turn_session: &mut Option<TurnSession>,
    pending_peer_commit: &mut Option<protocol::Commitment>,
    app: &mut App,
    conn: &mut Connection,
    local_side: &Side,
    kifu: &mut Kifu,
) -> Result<(), String> {
    match msg {
        NetMessage::Commit { commitment } => {
            let commit = commitment_from_hex(&commitment)
                .ok_or_else(|| "不正な commitment hex".to_string())?;

            if let Some(session) = turn_session.as_mut() {
                // 自分の着手確定後にコミットが届いた（通常ケース）
                session.receive_peer_commit(commit)
                    .map_err(|e| format!("commit 受信エラー: {:?}", e))?;

                if session.both_committed() {
                    let reveal = session.local_reveal()
                        .map_err(|e| format!("reveal 生成エラー: {:?}", e))?;
                    conn.send(&NetMessage::Reveal {
                        action_usi: reveal.action.to_usi(),
                        nonce: nonce_to_hex(&reveal.nonce),
                        board_hash: board_hash_to_hex(&reveal.board_hash),
                    }).map_err(|e| e.to_string())?;
                    *online_phase = OnlinePhase::WaitingPeerReveal;
                    app.message = "Reveal 送信済み — 相手の Reveal 待ち...".to_string();
                } else {
                    app.message = "相手のコミット受信済み — 自分の着手を確定してください".to_string();
                }
            } else {
                // 自分の着手確定前に相手のコミットが届いた（先着ケース）
                // → セッション生成まで保留し、着手確定後に適用する
                *pending_peer_commit = Some(commit);
                app.message = "相手のコミット受信済み — 着手を入力してください".to_string();
            }
        }

        NetMessage::Reveal { action_usi, nonce, board_hash } => {
            let peer_action = Action::from_usi(&action_usi)
                .ok_or_else(|| format!("不正な USI 文字列: {}", action_usi))?;
            let peer_nonce = nonce_from_hex(&nonce)
                .ok_or_else(|| "不正な nonce hex".to_string())?;
            let peer_hash = board_hash_from_hex(&board_hash)
                .ok_or_else(|| "不正な board_hash hex".to_string())?;

            let session = turn_session.as_mut()
                .ok_or_else(|| "セッション未初期化で Reveal 受信".to_string())?;

            session.receive_peer_reveal(peer_action, peer_nonce, peer_hash)
                .map_err(|e| format!("reveal 検証エラー: {:?}", e))?;

            // Ack 送信
            session.local_ack()
                .map_err(|e| format!("ack エラー: {:?}", e))?;
            conn.send(&NetMessage::Ack)
                .map_err(|e| e.to_string())?;
            *online_phase = OnlinePhase::WaitingPeerAck;
            app.message = "Ack 送信済み — 相手の Ack 待ち...".to_string();
        }

        NetMessage::Ack => {
            let session = turn_session.as_mut()
                .ok_or_else(|| "セッション未初期化で Ack 受信".to_string())?;

            session.receive_peer_ack();

            if session.is_complete() {
                let (sente_action, gote_action) = session.get_actions()
                    .ok_or_else(|| "ターン確定後に着手ペアなし".to_string())?;

                // kifu に記録（position の进行は resolve_turn が行う）
                app.sente_action = Some(sente_action);
                app.gote_action = Some(gote_action);
                app.resolve_turn();

                // kifu オブジェクトも同期
                use engine::types::Ply;
                kifu.push(Ply { sente: sente_action, gote: gote_action });

                *turn_session = None;

                if !matches!(app.phase, Phase::GameOver(_)) {
                    // 次ターンへ
                    *online_phase = OnlinePhase::WaitingMyMove;
                    match local_side {
                        Side::Gote => {
                            // resolve_turn は常に SenteInput に戻すので上書き
                            app.phase = Phase::GoteInput;
                            app.cursor_rank = 1;
                        }
                        Side::Sente => {}
                    }
                    app.message = "次の着手を入力してください".to_string();
                }
                // else: ゲーム終了 → GameOver フェーズはそのまま
            }
        }

        NetMessage::Abort { reason } => {
            return Err(format!("相手がアボート: {}", reason));
        }

        // GameStart / Reconnect はここには来ない（接続時に処理済み）
        NetMessage::GameStart { .. } => {}
        NetMessage::Reconnect { .. } => {}
    }
    Ok(())
}

// ─── WaitingMyMove でない時に Commit が先着した場合の対応 ────────────────────
// (相手が先にコミットしてくる可能性があるため、WaitingPeerCommit と
//  WaitingMyMove の両立が必要。上記 handle_net_message は session が
//  None のとき受信した Commit を「セッション未初期化」エラーとする。
//  より堅牢にするには、事前に Commit を pending として保持し、
//  セッションが生成されてから適用する。ここでは最小実装として
//  セッション生成後に Commit が来る想定で動かす。)

// ─── ハンドシェイク補助 ─────────────────────────────────────────────────────

fn wait_game_start(conn: &mut Connection) -> io::Result<SecretHash> {
    // GameStart を受信するまでブロッキング待機（最大 30 秒）
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "GameStart タイムアウト"));
        }
        if let Ok(ev) = conn.events.try_recv() {
            if let NetEvent::Message(NetMessage::GameStart { secret_hash, .. }) = ev {
                let bytes = net::from_hex(&secret_hash)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad secret_hash hex"))?;
                if bytes.len() != 32 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "secret_hash length"));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                return Ok(SecretHash(arr));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ─── 再接続 ─────────────────────────────────────────────────────────────────

fn reconnect(
    config: &OnlineConfig,
    kifu: &Kifu,
    peer_secret_hash: &SecretHash,
) -> io::Result<Connection> {
    // Connect 側は Listen 側の準備が整うまでリトライする
    let mut conn = match &config.mode {
        ConnectMode::Listen(port) => Connection::listen(*port)?,
        ConnectMode::Connect(addr) => {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            loop {
                match Connection::connect(addr) {
                    Ok(c) => break c,
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    };

    let current_hash = board_hash(&kifu.current());

    // 自分の秘密を送る（相手が SHA-256 して照合する）
    conn.send(&NetMessage::Reconnect {
        secret: net::to_hex(&config.secret),
        resume_hash: board_hash_to_hex(&current_hash),
    })?;

    // 相手の Reconnect を待つ
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Reconnect タイムアウト"));
        }
        if let Ok(NetEvent::Message(NetMessage::Reconnect { secret, resume_hash })) = conn.events.try_recv() {
            // 本人認証
            let recovery = RecoverySession::new(kifu.clone(), *peer_secret_hash);
            let secret_bytes = net::from_hex(&secret)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad secret hex"))?;
            if !recovery.verify_identity(&secret_bytes) {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "再接続: 認証失敗"));
            }
            // 盤面ハッシュ照合
            let peer_hash = board_hash_from_hex(&resume_hash)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad resume_hash hex"))?;
            if recovery.find_resume_point(peer_hash).is_none() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "再開点が一致しません"));
            }
            return Ok(conn);
        }
        std::thread::sleep(Duration::from_millis(50));
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
/// 毎回の状態遷移後に呼ぶこと。
fn sync_online_status(app: &mut App, phase: &OnlinePhase, local_side: Side, connected: bool) {
    let protocol = match phase {
        OnlinePhase::WaitingMyMove     => OnlineProtocolPhase::MyTurn,
        OnlinePhase::WaitingPeerCommit => OnlineProtocolPhase::PeerCommitPending,
        OnlinePhase::WaitingPeerReveal => OnlineProtocolPhase::PeerRevealPending,
        OnlinePhase::WaitingPeerAck    => OnlineProtocolPhase::PeerAckPending,
        OnlinePhase::Disconnected      => OnlineProtocolPhase::Disconnected,
        OnlinePhase::Aborted(r)        => OnlineProtocolPhase::Aborted(r.clone()),
    };
    // peer_revealed: ピアの着手が app に格納済みかで判定
    let peer_revealed = match local_side {
        Side::Sente => app.gote_action.is_some(),
        Side::Gote  => app.sente_action.is_some(),
    };
    app.online_status = Some(OnlineStatus { local_side, protocol, connected, peer_revealed });
}
