/// 通信秘匿対戦モード
///
/// commit-reveal-ack プロトコルを `ClientSession`（protocol クレート）に委譲しつつ、
/// TCP I/O を `Connection` に委譲する。
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
use crate::net::{Connection, NetEvent};
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

    // ── ハンドシェイク（hello 集約。版交渉は feed(Hello) の中）───────────────
    let mut session = ClientSession::new(config.local_side, &config.secret);
    conn.send(&session.hello_msg())?;
    let version_err: Option<String> = wait_and_feed_hello(&mut conn, &mut session).err();

    // ── 対局準備 ────────────────────────────────────────────────────────────
    let mut app = App::new();
    // 後手側はカーソルを段 1 から開始
    if config.local_side == Side::Gote {
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
            if config.local_side == Side::Sente {
                "先手: "
            } else {
                "後手: "
            }
        );
        OnlinePhase::WaitingMyMove
    };
    let mut kifu = Kifu::new(Position::initial());
    // 再接続バックグラウンドスレッドからの通知チャネル
    let mut reconnect_rx: Option<std::sync::mpsc::Receiver<ReconnectEvent>> = None;
    // 切断時点で着手が確定済みだったか（再接続後のロールバック通知に使う）
    let mut move_rolled_back = false;

    // 初期状態を online_status に反映（版交渉失敗時は切断扱い）
    sync_online_status(
        &mut app,
        &online_phase,
        config.local_side,
        version_err.is_none(),
    );

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
                        // 自分の現局面ハッシュで reconnect を送る（auth_hash は session が入れる）。
                        let bh = board_hash(&kifu.current());
                        let _ = conn.send(&session.reconnect_msg(bh));
                        online_phase = OnlinePhase::WaitingMyMove;
                        app.sente_action = None;
                        app.gote_action = None;
                        match config.local_side {
                            Side::Sente => {
                                app.phase = Phase::SenteInput;
                            }
                            Side::Gote => {
                                app.phase = Phase::GoteInput;
                                app.cursor_rank = 1;
                            }
                        }
                        app.message = if move_rolled_back {
                            move_rolled_back = false;
                            "着手をキャンセルしました — 再度入力してください".to_string()
                        } else {
                            "着手を入力してください".to_string()
                        };
                        sync_online_status(&mut app, &online_phase, config.local_side, true);
                        notify_reconnect(&mut app, 4);
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
        while let Ok(ev) = conn.events.try_recv() {
            match ev {
                NetEvent::Disconnected => {
                    if online_phase != OnlinePhase::Disconnected {
                        // 切断時点で着手が確定済みかを記録（再接続後のロールバック通知用）
                        let my_action = match config.local_side {
                            Side::Sente => app.sente_action,
                            Side::Gote => app.gote_action,
                        };
                        move_rolled_back = my_action.is_some();

                        // 初回切断時のみスレッド起動（二重起動防止）
                        online_phase = OnlinePhase::Disconnected;
                        session.abort_turn();
                        app.message = "接続が切断されました — 再接続中...".to_string();
                        sync_online_status(&mut app, &online_phase, config.local_side, false);

                        // バックグラウンドで再接続（TUI はブロックしない）。
                        // ソケット再確立のみ担う——Reconnect 交換はメインループが
                        // 永続 session で駆動する（R1）。
                        let config2 = config.clone();
                        let (tx, rx) = std::sync::mpsc::channel::<ReconnectEvent>();
                        reconnect_rx = Some(rx);
                        std::thread::spawn(move || {
                            let result = reconnect_socket_only(&config2);
                            let ev = match result {
                                Ok(conn) => ReconnectEvent::Success(conn),
                                Err(e) => ReconnectEvent::Failed(e.to_string()),
                            };
                            let _ = tx.send(ev);
                        });
                    }
                }
                NetEvent::Message(wire) => {
                    match session.feed(wire) {
                        Ok(SessionEvent::PeerCommitted { both_committed }) => {
                            if both_committed {
                                match session.reveal_msg() {
                                    Ok(reveal) => {
                                        conn.send(&reveal)?;
                                        online_phase = OnlinePhase::WaitingPeerReveal;
                                        app.message =
                                            "Reveal 送信済み — 相手の Reveal 待ち...".to_string();
                                    }
                                    Err(e) => abort(
                                        &mut online_phase,
                                        &mut conn,
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
                                        conn.send(&ack)?;
                                        online_phase = OnlinePhase::WaitingPeerAck;
                                        app.message =
                                            "Ack 送信済み — 相手の Ack 待ち...".to_string();
                                    }
                                    Err(e) => abort(
                                        &mut online_phase,
                                        &mut conn,
                                        format!("ack エラー: {:?}", e),
                                    ),
                                }
                            }
                        }
                        Ok(SessionEvent::TurnComplete { sente, gote }) => {
                            resolve_completed_turn(
                                sente,
                                gote,
                                &mut app,
                                &mut kifu,
                                &mut online_phase,
                                config.local_side,
                            );
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
                                let _ = conn.send(&session.reconnect_ack_msg(bh));
                            } else {
                                abort(&mut online_phase, &mut conn, "hash_mismatch".to_string());
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
                            abort(&mut online_phase, &mut conn, "auth_mismatch".to_string());
                        }
                        Err(e) => {
                            abort(
                                &mut online_phase,
                                &mut conn,
                                format!("プロトコルエラー: {:?}", e),
                            );
                        }
                        Ok(SessionEvent::HandshakeDone { .. }) | Ok(SessionEvent::PeerAcked) => {
                            // ループ中は無視/待機
                        }
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
                        match config.local_side {
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
        let my_action = match config.local_side {
            Side::Sente => app.sente_action,
            Side::Gote => app.gote_action,
        };

        if let Some(action) = my_action {
            // UI を "待機中" に固定
            app.phase = Phase::ResolveReady;
            let pos = kifu.current();
            let la = legal_actions(&pos, config.local_side);
            let notation = ja_notation(&action, config.local_side, &la, &pos);
            app.message = format!("着手確定: {}", notation);

            // commit-reveal セッション開始
            let pos = kifu.current();
            let pos_hash = board_hash(&pos);
            let nonce = random_nonce();
            match session.commit(pos_hash, action, nonce) {
                Ok(commit_msg) => {
                    conn.send(&commit_msg)?;
                    online_phase = OnlinePhase::WaitingPeerCommit;
                    sync_online_status(&mut app, &online_phase, config.local_side, true);

                    // 先着していた peer commit があれば commit() の中で適用済み
                    // → 両者揃っていれば即 reveal
                    if session.both_committed() {
                        let reveal = session
                            .reveal_msg()
                            .expect("both_committed 済みなら reveal 可");
                        conn.send(&reveal)?;
                        online_phase = OnlinePhase::WaitingPeerReveal;
                        sync_online_status(&mut app, &online_phase, config.local_side, true);
                    }
                }
                Err(e) => {
                    let reason = format!("commit エラー: {:?}", e);
                    online_phase = OnlinePhase::Aborted(reason.clone());
                    let _ = conn.send(&WireMessage::Abort { reason });
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
    local_side: Side,
) {
    // 投了判定（ルール 5.3 / 5.4）: resolve を通さず直接終局へ
    let s_resign = sente_action.is_resign();
    let g_resign = gote_action.is_resign();
    if s_resign || g_resign {
        use crate::app::{DrawReason, GameOverKind, WinReason};
        let kind = match (s_resign, g_resign) {
            (true, true) => GameOverKind::Draw(DrawReason::MutualResign),
            (true, false) => GameOverKind::GoteWins(WinReason::Resign),
            (false, true) => GameOverKind::SenteWins(WinReason::Resign),
            _ => unreachable!(),
        };
        app.phase = Phase::GameOver(kind);
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
        if local_side == Side::Gote {
            // resolve_turn は常に SenteInput に戻すので上書き
            app.phase = Phase::GoteInput;
            app.cursor_rank = 1;
        }
        app.message = "次の着手を入力してください".to_string();
    }
}

// ─── アボートの小さなヘルパ ─────────────────────────────────────────────────

/// 自分の判定でアボートし、相手にも通知する（Abort を送る）。
fn abort(online_phase: &mut OnlinePhase, conn: &mut Connection, reason: String) {
    *online_phase = OnlinePhase::Aborted(reason.clone());
    let _ = conn.send(&WireMessage::Abort { reason });
}

/// 相手が既にアボート済み、または通知不要な場合の局所反映のみ。
fn abort_to(online_phase: &mut OnlinePhase, reason: String) {
    *online_phase = OnlinePhase::Aborted(reason);
}

// ─── ハンドシェイク補助 ─────────────────────────────────────────────────────

/// peer の Hello を待って feed する。成功で peer_side、失敗で整形済みエラー文字列。
fn wait_and_feed_hello(conn: &mut Connection, session: &mut ClientSession) -> Result<Side, String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(
                "版交渉: 相手が応答しませんでした（版交渉未対応の版かもしれません）".to_string(),
            );
        }
        match conn.events.try_recv() {
            Ok(NetEvent::Message(wire @ WireMessage::Hello { .. })) => {
                return match session.feed(wire) {
                    Ok(SessionEvent::HandshakeDone { peer_side }) => Ok(peer_side),
                    Err(SessionError::VersionMismatch(o)) => Err(format_version_mismatch(&o)),
                    Err(e) => Err(format!("ハンドシェイク失敗: {:?}", e)),
                    Ok(_) => Err("ハンドシェイク: 予期しない応答".to_string()),
                };
            }
            Ok(NetEvent::Message(_)) => return Err("ハンドシェイク: hello 以外を受信".to_string()),
            Ok(NetEvent::Disconnected) => return Err("ハンドシェイク中に切断".to_string()),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

// ─── 再接続（ソケット再確立のみ。Reconnect 交換はメインループが担う・R1）──────

fn reconnect_socket_only(config: &OnlineConfig) -> io::Result<Connection> {
    match &config.mode {
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
