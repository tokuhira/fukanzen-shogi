use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use engine::types::{PieceKind, Side, Square};
use crate::app::{App, FocusArea, InputMode, Phase, Selection, HAND_KINDS};

// ─── キー入力ハンドラ ─────────────────────────────────────────────────────────

/// true を返すとアプリ終了
pub fn handle_key(key: KeyEvent, app: &mut App) -> bool {
    // パス入力モード中は別ハンドラへ
    if app.input_mode != InputMode::Normal {
        return handle_path_input(key, app);
    }

    use KeyCode::*;

    // Ctrl+C は常に終了
    if key.code == Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    // ゲームオーバー中の特別キー
    if let Phase::GameOver(_) = &app.phase {
        match key.code {
            Char('q') | Char('Q') => return true,
            Char('n') | Char('N') => { app.new_game(); return false; }
            Char('u') | Char('U') => { app.undo(); return false; }
            _ => return false,
        }
    }

    // 成り選択ダイアログ中
    if let Phase::PromotionChoice { .. } = &app.phase {
        match key.code {
            Char('y') | Char('p') | Char('P') | Enter => { app.apply_promotion(true); }
            Char('n') | Char('N') => { app.apply_promotion(false); }
            Esc => { app.cancel_promotion(); }
            _ => {}
        }
        return false;
    }

    // 通常操作
    match key.code {
        // 終了
        Char('q') | Char('Q') => return true,

        // キャンセル・選択解除
        Esc => { app.on_escape(); }

        // 決定
        Enter | Char(' ') => { app.on_enter(); }

        // カーソル移動 / 駒台カーソル移動
        Up => {
            if app.focus == FocusArea::Board {
                app.move_cursor(0, -1);
            } else {
                app.move_hand_cursor(-1);
            }
        }
        Down => {
            if app.focus == FocusArea::Board {
                app.move_cursor(0, 1);
            } else {
                app.move_hand_cursor(1);
            }
        }
        // 表示上: 左→筋番号大（file+1）、右→筋番号小（file-1）
        Left => {
            if app.focus == FocusArea::Board {
                app.move_cursor(1, 0);
            } else {
                app.move_hand_cursor(-1);
            }
        }
        Right => {
            if app.focus == FocusArea::Board {
                app.move_cursor(-1, 0);
            } else {
                app.move_hand_cursor(1);
            }
        }

        // 駒台切替
        Tab | Char('d') | Char('D') => { app.toggle_hand_focus(); }

        // 成り（プロモーション選択時以外は無視 — 上の Phase チェックで既に処理済み）
        Char('y') | Char('p') | Char('P') => {}
        Char('n') => {}

        // 補助操作
        Char('u') | Char('U') => { app.undo(); }
        Char('r') | Char('R') => { app.resign(); }

        // 保存・読込（小文字=デフォルトパス、大文字=パス入力）
        Char('s') => { app.save("shogi_game.kifu"); }
        Char('S') => {
            app.input_mode = InputMode::SavePath;
            app.input_buffer.clear();
        }
        Char('l') => { app.load("shogi_game.kifu"); }
        Char('L') => {
            app.input_mode = InputMode::LoadPath;
            app.input_buffer.clear();
        }

        // SFEN 表示
        Char('f') | Char('F') => { app.toggle_sfen(); }

        // 合法手一覧
        Char('m') | Char('M') => { app.toggle_all_moves(); }

        // ヘルプ
        Char('?') | Char('h') => { app.show_help = !app.show_help; }

        // 数字キーで持ち駒を直接選択（1=歩 2=香 3=桂 4=銀 5=金 6=角 7=飛）
        Char(c) if ('1'..='7').contains(&c) => {
            let idx = (c as usize) - ('1' as usize);
            if idx < HAND_KINDS.len() {
                app.select_hand_piece_direct(HAND_KINDS[idx]);
            }
        }

        _ => {}
    }
    false
}

// ─── パス入力モード ──────────────────────────────────────────────────────────

fn handle_path_input(key: KeyEvent, app: &mut App) -> bool {
    use KeyCode::*;
    match key.code {
        Enter => {
            let path = app.input_buffer.trim().to_string();
            match app.input_mode.clone() {
                InputMode::SavePath => {
                    app.input_mode = InputMode::Normal;
                    app.save(&path);
                }
                InputMode::LoadPath => {
                    app.input_mode = InputMode::Normal;
                    app.load(&path);
                }
                InputMode::Normal => {}
            }
        }
        Esc => {
            app.input_mode = InputMode::Normal;
            app.input_buffer.clear();
            app.message = "キャンセルしました".to_string();
        }
        Backspace => {
            app.input_buffer.pop();
        }
        Char(c) => {
            app.input_buffer.push(c);
        }
        _ => {}
    }
    false
}

// ─── マウス入力ハンドラ ──────────────────────────────────────────────────────

/// true を返すとアプリ終了（handle_key と同じ規約）
pub fn handle_mouse(mouse: MouseEvent, app: &mut App) -> bool {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
        return false;
    }

    let col = mouse.column;
    let row = mouse.row;

    // クリックが Rect 内に入っているか
    let hit = |r: Rect| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height;

    // ─── フェーズ別にオーバーレイを優先処理 ─────────────────────────────────

    // 成り選択ダイアログ（ポップアップが開いている間は他のクリックを無視）
    if matches!(app.phase, Phase::PromotionChoice { .. }) {
        if let Some(r) = app.click_areas.promote_yes { if hit(r) { app.apply_promotion(true);  return false; } }
        if let Some(r) = app.click_areas.promote_no  { if hit(r) { app.apply_promotion(false); return false; } }
        return false; // ダイアログ外クリックは無視
    }

    // ゲームオーバーダイアログ
    if let Phase::GameOver(_) = &app.phase {
        if let Some(r) = app.click_areas.gameover_undo { if hit(r) { app.undo();     return false; } }
        if let Some(r) = app.click_areas.gameover_new  { if hit(r) { app.new_game(); return false; } }
        if let Some(r) = app.click_areas.gameover_quit { if hit(r) { return true; } }
        return false;
    }

    // ─── 通常操作 ────────────────────────────────────────────────────────────

    // 解決ボタン（ResolveReady 時のステータス行）
    if let Some(r) = app.click_areas.resolve {
        if hit(r) { app.resolve_turn(); return false; }
    }

    // 駒台の持ち駒を直接クリックして選択
    let matched_hand: Option<PieceKind> = match app.current_side() {
        None => None,
        Some(side) => {
            let rects = if side == Side::Sente {
                &app.click_areas.sente_hand
            } else {
                &app.click_areas.gote_hand
            };
            rects.iter().find(|(r, _)| hit(*r)).map(|(_, k)| *k)
        }
    };
    if let Some(kind) = matched_hand {
        // 選択済みの同じ駒をクリックしたら解除（盤面駒の挙動と対称）
        if matches!(&app.selection, Selection::HandPiece(k) if *k == kind) {
            app.clear_selection();
            app.message = "選択解除".to_string();
        } else {
            app.select_hand_piece_direct(kind);
        }
        return false;
    }

    // ─── 盤面クリック ────────────────────────────────────────────────────────

    // 盤面ブロック inner 領域: x=1, y=1, inner rows 0..11
    // 縦分割: [Min(13), 1, 2, 1, 1] → main_area は y=0 から始まる
    const STATUS_ROWS: u16 = 1 + 2 + 1 + 1;
    let (_, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
    let main_h = term_h.saturating_sub(STATUS_ROWS);

    let bx = 1u16;
    let by = 1u16;
    const BOARD_PANEL_W: u16 = 33;

    if col < bx || col >= bx + BOARD_PANEL_W - 2 { return false; }
    if row < by || row >= by + main_h.saturating_sub(2) { return false; }

    let local_row = row - by;

    match local_row {
        r if (2..=10).contains(&r) => {
            let rank = (r - 1) as u8;
            let local_col = col.saturating_sub(bx + 2);
            let cell_idx = local_col / 3;
            if cell_idx < 9 {
                let file = 9u8.saturating_sub(cell_idx as u8);
                if (1..=9).contains(&file) {
                    let sq = Square::new(file, rank);
                    app.cursor_file = file;
                    app.cursor_rank = rank;
                    if let Some(side) = app.current_side() {
                        app.on_board_press(sq, side);
                    }
                }
            }
        }
        _ => {}
    }

    false
}
