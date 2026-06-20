use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use engine::types::{Side, Square};
use crate::app::{App, FocusArea, InputMode, Phase, HAND_KINDS};

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

/// 画面レイアウト定数（ui.rs のレイアウトと同期）
/// 縦分割: [Min(13), Length(1), Length(2), Length(1), Length(1)]
/// 横分割: [Length(33), Min(1)]
const BOARD_PANEL_W: u16 = 33;
const STATUS_ROWS: u16 = 1 + 2 + 1 + 1; // ステータス+仮着手+メッセージ+ヘルプ = 5行

pub fn handle_mouse(mouse: MouseEvent, app: &mut App) {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
        return;
    }

    let col = mouse.column;
    let row = mouse.row;

    // 端末サイズを取得してメインエリアの高さを計算
    let (_, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
    let main_h = term_h.saturating_sub(STATUS_ROWS);

    // 盤面ブロック (borders=ALL): area(0, 0, 33, main_h)
    // inner area: (1, 1, 31, main_h-2)
    let bx = 1u16; // inner x
    let by = 1u16; // inner y

    if col < bx || col >= bx + BOARD_PANEL_W - 2 {
        return; // 盤面パネル外
    }
    if row < by || row >= by + main_h.saturating_sub(2) {
        return;
    }

    let local_row = row - by;

    // inner 内の行レイアウト:
    //   row 0: 後手持駒
    //   row 1: 列ヘッダ
    //   row 2..10: 段 1..9
    //   row 11: 先手持駒
    match local_row {
        0 => {
            // 後手持駒クリック
            if app.current_side() == Some(Side::Gote) {
                app.toggle_hand_focus();
            }
        }
        11 => {
            // 先手持駒クリック
            if app.current_side() == Some(Side::Sente) {
                app.toggle_hand_focus();
            }
        }
        r if (2..=10).contains(&r) => {
            let rank = (r - 1) as u8; // row2→rank1 ... row10→rank9
            let local_col = col.saturating_sub(bx + 2); // rank_label(2cols) を除く
            let cell_idx = local_col / 3; // 各セル 3 display cols

            if cell_idx < 9 {
                // 表示上: cell_idx 0 = 筋9, cell_idx 8 = 筋1
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
}
