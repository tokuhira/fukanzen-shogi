use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use engine::board::Hand;
use engine::types::{PieceKind, Side, Square};
use crate::app::{
    App, ClickAreas, FocusArea, GameOverKind, InputMode, Phase, Selection,
    game_over_text, piece_kind_ja, HAND_KINDS,
};

// ─── 全画面描画エントリ ───────────────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // 縦分割: メイン / ステータス / 仮着手表示 / メッセージ / ヘルプ行
    let vchunks = Layout::vertical([
        Constraint::Min(13),   // メイン（盤面 + 情報）
        Constraint::Length(1), // ステータス行
        Constraint::Length(2), // 仮確定着手
        Constraint::Length(1), // メッセージ
        Constraint::Length(1), // ヘルプ短縮表示
    ])
    .split(area);

    let main_area    = vchunks[0];
    let status_area  = vchunks[1];
    let pending_area = vchunks[2];
    let msg_area     = vchunks[3];
    let help_area    = vchunks[4];

    // 横分割: 盤面 | 情報パネル
    let board_panel_w = 33u16;
    let hchunks = Layout::horizontal([
        Constraint::Length(board_panel_w),
        Constraint::Min(1),
    ])
    .split(main_area);

    let board_area = hchunks[0];
    let info_area  = hchunks[1];

    // ─── クリック領域を毎フレーム更新 ────────────────────────────────────────
    app.click_areas = ClickAreas::default();

    // 解決ボタン: ResolveReady のときステータス行全体をクリック可能にする
    if matches!(app.phase, Phase::ResolveReady) {
        app.click_areas.resolve = Some(status_area);
    }

    // 成り選択ポップアップのボタン領域（左半分=成る、右半分=成らない）
    if matches!(app.phase, Phase::PromotionChoice { .. }) {
        let pw = 38u16.min(area.width.saturating_sub(4));
        let pp = centered_rect(pw, 5, area);
        let ix  = pp.x + 1;
        let iy  = pp.y + 1;
        let iw  = pp.width.saturating_sub(2);
        let btn_y = iy + 1; // inner row 0 = blank、row 1 = ボタン行
        let half = iw / 2;
        app.click_areas.promote_yes = Some(Rect::new(ix,        btn_y, half,      1));
        app.click_areas.promote_no  = Some(Rect::new(ix + half, btn_y, iw - half, 1));
    }

    // ゲームオーバーポップアップのボタン行（inner rows 3/4/5）
    if let Phase::GameOver(_) = &app.phase {
        let pw = 44u16.min(area.width.saturating_sub(4));
        let pp = centered_rect(pw, 8, area);
        let ix = pp.x + 1;
        let iy = pp.y + 1;
        let iw = pp.width.saturating_sub(2);
        app.click_areas.gameover_undo = Some(Rect::new(ix, iy + 3, iw, 1));
        app.click_areas.gameover_new  = Some(Rect::new(ix, iy + 4, iw, 1));
        app.click_areas.gameover_quit = Some(Rect::new(ix, iy + 5, iw, 1));
    }

    // 駒台の持ち駒領域（"先手持駒: " 等の接頭辞 10 cols の直後から各駒を配置）
    {
        let pos = app.current_pos();
        let bix = board_area.x + 1; // board ブロック inner の x
        let biy = board_area.y + 1; // board ブロック inner の y
        app.click_areas.gote_hand  = hand_piece_rects(&pos.hand_gote,  bix, biy);
        app.click_areas.sente_hand = hand_piece_rects(&pos.hand_sente, bix, biy + 11);
    }

    render_board(f, app, board_area);
    render_info(f, app, info_area);
    render_status(f, app, status_area);
    render_pending(f, app, pending_area);
    render_message(f, app, msg_area);
    render_help_bar(f, help_area);

    // オーバーレイ（後から描くほど前面）
    if matches!(app.phase, Phase::PromotionChoice { .. }) {
        render_promotion_popup(f, area);
    }
    if let Phase::GameOver(ref kind) = app.phase {
        render_game_over_popup(f, kind, area);
    }
    if app.show_help {
        render_help_popup(f, area);
    }
}

// ─── 駒台クリック領域計算 ─────────────────────────────────────────────────────

fn hand_piece_rects(hand: &Hand, base_x: u16, base_y: u16) -> Vec<(Rect, PieceKind)> {
    // "先手持駒: " / "後手持駒: " のプレフィックスは 10 display cols
    let mut x = base_x + 10;
    let mut v = Vec::new();
    for kind in HAND_KINDS.iter().copied() {
        let cnt = hand.count(kind);
        if cnt == 0 { continue; }
        // cnt>1 → "{漢字}{数字} " = 4 cols、cnt==1 → "{漢字} " = 3 cols
        let w = if cnt > 1 { 4u16 } else { 3u16 };
        v.push((Rect::new(x, base_y, w, 1), kind));
        x += w;
    }
    v
}

// ─── 盤面描画 ─────────────────────────────────────────────────────────────────

fn render_board(f: &mut Frame, app: &mut App, area: Rect) {
    let pos = app.current_pos();

    let block = Block::default()
        .borders(Borders::ALL)
        .title("盤面  [d/Tab]駒台切替");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // 後手持駒行
    lines.push(hand_line(&pos.hand_gote, Side::Gote, app));

    // 列ヘッダ: "  ９ ８ ７ ６ ５ ４ ３ ２ １"
    // rank label area = 2 display cols, each cell = 3 display cols
    let mut header_spans: Vec<Span> = vec![Span::raw("  ")];
    for file in (1u8..=9).rev() {
        header_spans.push(Span::styled(
            format!(" {}", WIDE_DIGIT[file as usize - 1]),
            Style::default().fg(Color::DarkGray),
        ));
    }
    lines.push(Line::from(header_spans));

    // 盤面行
    for rank in 1u8..=9 {
        let rank_char = (b'a' + rank - 1) as char;
        let mut spans: Vec<Span> = vec![
            Span::styled(
                format!("{} ", rank_char),
                Style::default().fg(Color::DarkGray),
            ),
        ];

        for file in (1u8..=9).rev() {
            let sq = Square::new(file, rank);
            let is_cursor = app.cursor_file == file && app.cursor_rank == rank;
            let is_from = match &app.selection {
                Selection::BoardPiece(s) => *s == sq,
                _ => false,
            };
            let is_highlight = app.highlights.contains(&sq);

            let (cell_text, piece_color) = match pos.board.get(sq) {
                None => (" . ".to_string(), Color::DarkGray),
                Some(p) => {
                    let prefix = if p.side == Side::Sente { ' ' } else { 'v' };
                    (
                        format!("{}{}", prefix, piece_kind_ja(p.kind)),
                        if p.side == Side::Sente {
                            Color::White
                        } else {
                            Color::LightRed
                        },
                    )
                }
            };

            let style = cell_style(is_cursor, is_from, is_highlight, piece_color);
            spans.push(Span::styled(cell_text, style));
        }

        lines.push(Line::from(spans));
    }

    // 先手持駒行
    lines.push(hand_line(&pos.hand_sente, Side::Sente, app));

    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn cell_style(is_cursor: bool, is_from: bool, is_highlight: bool, piece_fg: Color) -> Style {
    let base = Style::default().fg(piece_fg);
    if is_from {
        base.bg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else if is_highlight && is_cursor {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else if is_highlight {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else if is_cursor {
        base.add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
    } else {
        base
    }
}

fn hand_line<'a>(hand: &Hand, side: Side, app: &'a App) -> Line<'a> {
    let label = match side {
        Side::Sente => "先手持駒: ",
        Side::Gote  => "後手持駒: ",
    };
    let label_style = Style::default().add_modifier(Modifier::BOLD);
    let mut spans: Vec<Span<'a>> = vec![Span::styled(label.to_string(), label_style)];

    let is_current_side = app.current_side() == Some(side);
    let mut any = false;

    for kind in HAND_KINDS.iter().copied() {
        let cnt = hand.count(kind);
        if cnt == 0 {
            continue;
        }
        let is_selected = is_current_side
            && matches!(&app.selection, Selection::HandPiece(k) if *k == kind);

        let text = if cnt > 1 {
            format!("{}{} ", piece_kind_ja(kind), cnt)
        } else {
            format!("{} ", piece_kind_ja(kind))
        };

        let style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(if side == Side::Sente {
                Color::White
            } else {
                Color::LightRed
            })
        };
        spans.push(Span::styled(text, style));
        any = true;
    }
    if !any {
        spans.push(Span::styled("なし".to_string(), Style::default().fg(Color::DarkGray)));
    }

    // 駒台フォーカス中の表示
    if is_current_side && app.focus == FocusArea::Hand {
        spans.push(Span::styled(" ←→", Style::default().fg(Color::Yellow)));
    }

    Line::from(spans)
}

// ─── 情報パネル ───────────────────────────────────────────────────────────────

fn render_info(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title("情報");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // 直前の解決結果
    if !app.last_resolution.is_empty() {
        lines.push(Line::from(Span::styled(
            "◀ 直前の解決 ▶",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        for text in &app.last_resolution {
            lines.push(Line::from(Span::raw(text.clone())));
        }
        lines.push(Line::raw(""));
    }

    // SFEN 表示
    if app.show_sfen {
        lines.push(Line::from(Span::styled(
            "SFEN:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        // 長い SFEN を折り返す
        let sfen = &app.sfen_text;
        let w = inner.width.saturating_sub(1) as usize;
        if w > 0 {
            for chunk in sfen.as_bytes().chunks(w.max(1)) {
                lines.push(Line::raw(
                    String::from_utf8_lossy(chunk).to_string(),
                ));
            }
        } else {
            lines.push(Line::raw(sfen.clone()));
        }
        lines.push(Line::raw(""));
    }

    // 合法手一覧
    if app.show_all_moves {
        let side = app.current_side().unwrap_or(Side::Sente);
        let label = match side { Side::Sente => "先手", Side::Gote => "後手" };
        lines.push(Line::from(Span::styled(
            format!("{} 合法手 ({}手):", label, app.all_moves_text.len()),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        let cols = ((inner.width.saturating_sub(1)) / 6).max(1) as usize;
        for chunk in app.all_moves_text.chunks(cols) {
            lines.push(Line::raw(chunk.join(" ")));
        }
        lines.push(Line::raw(""));
    }

    // 組手数（move_number は SFEN の次手番号で 1 始まり → そのまま「第N組手目」）
    let kumite_num = app.kifu.current().move_number;
    lines.push(Line::from(vec![
        Span::styled("第", Style::default().fg(Color::DarkGray)),
        Span::raw(kumite_num.to_string()),
        Span::styled("組手", Style::default().fg(Color::DarkGray)),
    ]));

    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

// ─── ステータス行 ─────────────────────────────────────────────────────────────

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let (phase_text, phase_style) = match &app.phase {
        Phase::SenteInput => (
            "先手 入力中".to_string(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Phase::GoteInput => (
            "後手 入力中".to_string(),
            Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
        ),
        Phase::PromotionChoice { side, .. } => {
            let label = match side { Side::Sente => "先手", Side::Gote => "後手" };
            (
                format!("{} 成り選択中", label),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )
        }
        Phase::ResolveReady => (
            "▶ 両着手を同時解決 — Enter またはクリック".to_string(),
            Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD),
        ),
        Phase::GameOver(kind) => (
            format!("対局終了: {}", game_over_text(kind)),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ),
    };

    let line = Line::from(vec![
        Span::styled("◆ ", Style::default().fg(Color::DarkGray)),
        Span::styled(phase_text, phase_style),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

// ─── 仮確定着手表示 ───────────────────────────────────────────────────────────

fn render_pending(f: &mut Frame, app: &App, area: Rect) {
    let sente_str = match app.sente_action {
        Some(a) => a.to_usi(),
        None => "未入力".to_string(),
    };
    let gote_str = match app.gote_action {
        Some(a) => a.to_usi(),
        None => "未入力".to_string(),
    };

    let line1 = Line::from(vec![
        Span::styled("先手: ", Style::default().fg(Color::Cyan)),
        Span::raw(sente_str),
        Span::raw("   "),
        Span::styled("後手: ", Style::default().fg(Color::LightRed)),
        Span::raw(gote_str),
    ]);

    let cursor_info = match app.focus {
        FocusArea::Board => format!(
            "カーソル: {}{}  (↑↓←→で移動、Enter/Spaceで選択)",
            app.cursor_file,
            (b'a' + app.cursor_rank - 1) as char
        ),
        FocusArea::Hand => "駒台選択中 (←→で駒種切替、Enter確定、Esc中止)".to_string(),
    };
    let line2 = Line::from(Span::styled(cursor_info, Style::default().fg(Color::DarkGray)));

    f.render_widget(Paragraph::new(Text::from(vec![line1, line2])), area);
}

// ─── メッセージ行 ─────────────────────────────────────────────────────────────

fn render_message(f: &mut Frame, app: &App, area: Rect) {
    let text = if app.input_mode != InputMode::Normal {
        let prompt = match app.input_mode {
            InputMode::SavePath => "保存先パス: ",
            InputMode::LoadPath => "読み込みパス: ",
            InputMode::Normal => "",
        };
        format!("{}{}_", prompt, app.input_buffer)
    } else {
        app.message.clone()
    };

    let style = if app.input_mode != InputMode::Normal {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    f.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), area);
}

// ─── ヘルプ短縮バー ──────────────────────────────────────────────────────────

fn render_help_bar(f: &mut Frame, area: Rect) {
    let help = "[q]終了 [u]戻す [r]投了 [d/Tab]駒台 [s/S]保存 [l/L]読込 [f]SFEN [m]合法手 [?]ヘルプ";
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            help,
            Style::default().fg(Color::DarkGray),
        ))),
        area,
    );
}

// ─── 成りポップアップ ─────────────────────────────────────────────────────────

fn render_promotion_popup(f: &mut Frame, area: Rect) {
    let w = 38u16.min(area.width.saturating_sub(4));
    let h = 5u16;
    let popup = centered_rect(w, h, area);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("成りますか？")
        .style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // ボタン行: 左半分=成る（緑背景）、右半分=成らない（白背景）
    let btn_line = Line::from(vec![
        Span::styled(
            " 成る (y/p) ",
            Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD),
        ),
        Span::raw("    "),
        Span::styled(
            " 成らない (n) ",
            Style::default().fg(Color::Black).bg(Color::White),
        ),
    ]);
    let cancel_line = Line::from(Span::styled(
        "  [Esc] キャンセル",
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(
        Paragraph::new(Text::from(vec![Line::raw(""), btn_line, cancel_line])),
        inner,
    );
}

// ─── ゲームオーバーポップアップ ──────────────────────────────────────────────

fn render_game_over_popup(f: &mut Frame, kind: &GameOverKind, area: Rect) {
    let w = 44u16.min(area.width.saturating_sub(4));
    let h = 8u16;
    let popup = centered_rect(w, h, area);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("対局終了")
        .style(Style::default().fg(Color::Magenta));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let result_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let btn = |arrow: &'static str, color: Color, text: &'static str| {
        Line::from(vec![
            Span::styled(arrow, Style::default().fg(color)),
            Span::styled(text, Style::default().fg(Color::White)),
        ])
    };
    let lines: Vec<Line> = vec![
        Line::raw(""),
        Line::from(Span::styled(game_over_text(kind).to_string(), result_style)),
        Line::raw(""),
        btn("▶ ", Color::Green,   "[u] 最後の手を取り消して続行"),
        btn("▶ ", Color::Cyan,    "[n] 新規対局"),
        Line::from(Span::styled("  [q] 終了", Style::default().fg(Color::DarkGray))),
    ];
    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

// ─── ヘルプポップアップ ──────────────────────────────────────────────────────

fn render_help_popup(f: &mut Frame, area: Rect) {
    let w = 52u16.min(area.width.saturating_sub(4));
    let h = 24u16.min(area.height.saturating_sub(2));
    let popup = centered_rect(w, h, area);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("ヘルプ — [?]で閉じる")
        .style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let heading = |s: &'static str| {
        Line::from(Span::styled(
            s,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
    };
    let item = |s: &'static str| Line::from(Span::raw(s));

    let lines: Vec<Line> = vec![
        heading("── 移動 ──────────────────────────────────"),
        item("  ↑↓←→      カーソル移動（盤面上）"),
        item("  Enter/Space  選択または確定"),
        item("  Esc          選択解除・キャンセル"),
        heading("── 着手 ──────────────────────────────────"),
        item("  [d] または [Tab]  駒台選択モード切替"),
        item("     駒台モード: ←→ で駒種を選択"),
        item("     Enter で確定 → 盤面で打ち先を選択"),
        item("  [y]/[p]  成る（成り選択ダイアログ）"),
        item("  [n]       成らない"),
        heading("── ゲーム操作 ────────────────────────────"),
        item("  [u]  1ターン戻す（入力中は入力リセット）"),
        item("  [r]  現在フェーズの陣営が投了"),
        item("  [n]  新規対局（対局終了後のみ）"),
        heading("── ファイル ──────────────────────────────"),
        item("  [s]  保存（shogi_game.kifu）"),
        item("  [S]  保存（パス指定）"),
        item("  [l]  読込（shogi_game.kifu）"),
        item("  [L]  読込（パス指定）"),
        heading("── 表示 ──────────────────────────────────"),
        item("  [f]  現局面の SFEN 表示"),
        item("  [m]  合法手一覧表示"),
        item("  [?]  このヘルプ"),
        item("  [q]  終了"),
    ];

    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

// ─── ユーティリティ ──────────────────────────────────────────────────────────

fn centered_rect(w: u16, h: u16, r: Rect) -> Rect {
    let x = r.x + r.width.saturating_sub(w) / 2;
    let y = r.y + r.height.saturating_sub(h) / 2;
    Rect::new(x, y, w.min(r.width), h.min(r.height))
}

const WIDE_DIGIT: [&str; 9] = ["１", "２", "３", "４", "５", "６", "７", "８", "９"];
