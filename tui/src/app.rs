use std::collections::HashSet;
use ratatui::layout::Rect;
use engine::board::Position;
use engine::kifu::Kifu;
use engine::movegen::legal_actions;
use engine::resolve::{resolve, ResolutionEvent};
use engine::serialize::{kifu_from_string, kifu_to_string, position_to_sfen};
use engine::terminate::{check_king_death, check_sennichite, check_status, GameEnd, GameStatus};
use engine::types::{Action, PieceKind, Ply, Side, Square};

// ─── 公開定数 ────────────────────────────────────────────────────────────────

pub const HAND_KINDS: [PieceKind; 7] = [
    PieceKind::Pawn,
    PieceKind::Lance,
    PieceKind::Knight,
    PieceKind::Silver,
    PieceKind::Gold,
    PieceKind::Bishop,
    PieceKind::Rook,
];

// ─── 列挙型 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    SenteInput,
    GoteInput,
    PromotionChoice { from: Square, to: Square, side: Side },
    ResolveReady,
    GameOver(GameOverKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameOverKind {
    SenteWins(WinReason),
    GoteWins(WinReason),
    Draw(DrawReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WinReason {
    Resign,
    KingDied,
    Checkmate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawReason {
    BothKingDied,
    BothCheckmate,
    Sennichite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    BoardPiece(Square),
    HandPiece(PieceKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusArea {
    Board,
    Hand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    SavePath,
    LoadPath,
}

// ─── クリック領域キャッシュ ───────────────────────────────────────────────────

/// 毎フレーム ui::draw が更新する。input::handle_mouse がこれを参照する。
#[derive(Default, Clone)]
pub struct ClickAreas {
    pub promote_yes:   Option<Rect>, // 成りボタン（左半分）
    pub promote_no:    Option<Rect>, // 成らないボタン（右半分）
    pub resolve:       Option<Rect>, // 解決ボタン（ResolveReady 時のステータス行）
    pub gameover_undo: Option<Rect>, // ゲームオーバー: 一手戻す
    pub gameover_new:  Option<Rect>, // ゲームオーバー: 新規対局
    pub gameover_quit: Option<Rect>, // ゲームオーバー: 終了
    pub sente_hand:    Vec<(Rect, PieceKind)>, // 先手持ち駒の各駒
    pub gote_hand:     Vec<(Rect, PieceKind)>, // 後手持ち駒の各駒
}

// ─── App 状態 ─────────────────────────────────────────────────────────────────

pub struct App {
    pub kifu: Kifu,
    pub phase: Phase,
    pub sente_action: Option<Action>,
    pub gote_action: Option<Action>,
    // カーソル位置: 筋 1-9, 段 1-9
    pub cursor_file: u8,
    pub cursor_rank: u8,
    pub focus: FocusArea,
    pub hand_cursor: usize,
    pub selection: Selection,
    pub highlights: HashSet<Square>,
    pub last_resolution: Vec<String>,
    pub message: String,
    pub show_help: bool,
    pub show_sfen: bool,
    pub sfen_text: String,
    pub show_all_moves: bool,
    pub all_moves_text: Vec<String>,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub click_areas: ClickAreas,
}

impl App {
    pub fn new() -> Self {
        App {
            kifu: Kifu::new(Position::initial()),
            phase: Phase::SenteInput,
            sente_action: None,
            gote_action: None,
            cursor_file: 5,
            cursor_rank: 9,
            focus: FocusArea::Board,
            hand_cursor: 0,
            selection: Selection::None,
            highlights: HashSet::new(),
            last_resolution: Vec::new(),
            message: "'?'キーでヘルプを表示".to_string(),
            show_help: false,
            show_sfen: false,
            sfen_text: String::new(),
            show_all_moves: false,
            all_moves_text: Vec::new(),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            click_areas: ClickAreas::default(),
        }
    }

    pub fn current_pos(&self) -> Position {
        self.kifu.current()
    }

    pub fn current_side(&self) -> Option<Side> {
        match &self.phase {
            Phase::SenteInput => Some(Side::Sente),
            Phase::GoteInput => Some(Side::Gote),
            _ => None,
        }
    }

    pub fn cursor_sq(&self) -> Square {
        Square::new(self.cursor_file, self.cursor_rank)
    }

    pub fn move_cursor(&mut self, df: i8, dr: i8) {
        let new_file = (self.cursor_file as i8 + df).clamp(1, 9) as u8;
        let new_rank = (self.cursor_rank as i8 + dr).clamp(1, 9) as u8;
        self.cursor_file = new_file;
        self.cursor_rank = new_rank;
    }

    // ─── 入力フェーズのメイン処理 ────────────────────────────────────────────

    pub fn on_enter(&mut self) {
        match self.phase.clone() {
            Phase::SenteInput | Phase::GoteInput => {
                let side = self.current_side().unwrap();
                match self.focus.clone() {
                    FocusArea::Hand => self.confirm_hand_selection(),
                    FocusArea::Board => {
                        let sq = self.cursor_sq();
                        self.on_board_press(sq, side);
                    }
                }
            }
            Phase::ResolveReady => self.resolve_turn(),
            Phase::PromotionChoice { .. } => self.apply_promotion(true),
            Phase::GameOver(_) => {}
        }
    }

    pub fn on_board_press(&mut self, sq: Square, side: Side) {
        let pos = self.current_pos();

        match self.selection.clone() {
            Selection::None => {
                if let Some(piece) = pos.board.get(sq) {
                    if piece.side == side {
                        self.selection = Selection::BoardPiece(sq);
                        self.update_highlights();
                        self.message = format!(
                            "{}を選択 — 移動先を選んでください（Escでキャンセル）",
                            piece_kind_ja(piece.kind)
                        );
                    } else {
                        self.message = "自分の駒を選んでください".to_string();
                    }
                } else {
                    self.message = "その位置に駒がありません".to_string();
                }
            }
            Selection::BoardPiece(from) => {
                if sq == from {
                    self.clear_selection();
                    return;
                }
                if self.highlights.contains(&sq) {
                    self.confirm_move(from, sq, side);
                } else if let Some(piece) = pos.board.get(sq) {
                    if piece.side == side {
                        // 別の自駒を選び直す
                        self.selection = Selection::BoardPiece(sq);
                        self.update_highlights();
                        self.message = format!("{}を選択", piece_kind_ja(piece.kind));
                    } else {
                        self.message = "合法手ではありません".to_string();
                    }
                } else {
                    self.message = "合法手ではありません".to_string();
                }
            }
            Selection::HandPiece(kind) => {
                if self.highlights.contains(&sq) {
                    self.confirm_drop(kind, sq, side);
                } else {
                    self.message = "その位置には打てません".to_string();
                }
            }
        }
    }

    fn confirm_move(&mut self, from: Square, to: Square, side: Side) {
        let pos = self.current_pos();
        let actions = legal_actions(&pos, side);

        let can_promote = actions.iter().any(|a| {
            matches!(a, Action::Move { from: f, to: t, promote: true } if *f == from && *t == to)
        });
        let can_no_promote = actions.iter().any(|a| {
            matches!(a, Action::Move { from: f, to: t, promote: false } if *f == from && *t == to)
        });

        if can_promote && can_no_promote {
            self.clear_selection();
            self.phase = Phase::PromotionChoice { from, to, side };
            self.message = "[y]/[p]成る  [n]成らない  [Esc]キャンセル".to_string();
        } else {
            let action = Action::Move { from, to, promote: can_promote };
            self.apply_action(action, side);
        }
    }

    fn confirm_drop(&mut self, kind: PieceKind, to: Square, side: Side) {
        let action = Action::Drop { kind, to };
        self.apply_action(action, side);
    }

    pub fn apply_promotion(&mut self, promote: bool) {
        if let Phase::PromotionChoice { from, to, side } = self.phase.clone() {
            let action = Action::Move { from, to, promote };
            self.apply_action(action, side);
        }
    }

    pub fn cancel_promotion(&mut self) {
        if let Phase::PromotionChoice { side, .. } = self.phase.clone() {
            self.phase = match side {
                Side::Sente => Phase::SenteInput,
                Side::Gote => Phase::GoteInput,
            };
            self.message = "キャンセルしました".to_string();
        }
    }

    fn apply_action(&mut self, action: Action, side: Side) {
        self.clear_selection();
        match side {
            Side::Sente => {
                self.sente_action = Some(action);
                self.phase = Phase::GoteInput;
                self.cursor_file = 5;
                self.cursor_rank = 1;
                self.message = format!("先手: {} 確定。後手の着手を入力してください", action.to_usi());
            }
            Side::Gote => {
                self.gote_action = Some(action);
                self.phase = Phase::ResolveReady;
                self.message = format!("後手: {} 確定。[Enter]で解決", action.to_usi());
            }
        }
    }

    fn update_highlights(&mut self) {
        let pos = self.current_pos();
        let side = match self.current_side() {
            Some(s) => s,
            None => {
                self.highlights.clear();
                return;
            }
        };

        let actions = legal_actions(&pos, side);
        self.highlights = match &self.selection {
            Selection::BoardPiece(sq) => {
                let from = *sq;
                actions
                    .iter()
                    .filter(|a| a.from_sq() == Some(from))
                    .map(|a| a.to_sq())
                    .collect()
            }
            Selection::HandPiece(kind) => {
                let k = *kind;
                actions
                    .iter()
                    .filter(|a| matches!(a, Action::Drop { kind: ak, .. } if *ak == k))
                    .map(|a| a.to_sq())
                    .collect()
            }
            Selection::None => HashSet::new(),
        };
    }

    pub fn clear_selection(&mut self) {
        self.selection = Selection::None;
        self.highlights.clear();
        self.focus = FocusArea::Board;
    }

    // ─── 駒台選択 ─────────────────────────────────────────────────────────────

    pub fn toggle_hand_focus(&mut self) {
        let side = match self.current_side() {
            Some(s) => s,
            None => return,
        };

        if self.focus == FocusArea::Hand {
            self.focus = FocusArea::Board;
            self.clear_selection();
            return;
        }

        let pos = self.current_pos();
        let hand = pos.hand(side);
        let pieces: Vec<PieceKind> = HAND_KINDS.iter().copied().filter(|&k| hand.has(k)).collect();

        if pieces.is_empty() {
            self.message = "持ち駒がありません".to_string();
            return;
        }

        self.focus = FocusArea::Hand;
        self.hand_cursor = 0;
        let kind = pieces[0];
        self.selection = Selection::HandPiece(kind);
        self.update_highlights();
        self.message = format!("駒台: {}選択中 — ←→で変更、Enterで確定、Escでキャンセル", piece_kind_ja(kind));
    }

    pub fn move_hand_cursor(&mut self, dir: i8) {
        let side = match self.current_side() {
            Some(s) => s,
            None => return,
        };
        let pos = self.current_pos();
        let hand = pos.hand(side);
        let pieces: Vec<PieceKind> = HAND_KINDS.iter().copied().filter(|&k| hand.has(k)).collect();
        if pieces.is_empty() {
            return;
        }

        let len = pieces.len() as i8;
        self.hand_cursor = ((self.hand_cursor as i8 + dir).rem_euclid(len)) as usize;
        let kind = pieces[self.hand_cursor];
        self.selection = Selection::HandPiece(kind);
        self.update_highlights();
        self.message = format!("駒台: {}選択中 — ←→で変更、Enterで確定", piece_kind_ja(kind));
    }

    pub fn select_hand_piece_direct(&mut self, kind: PieceKind) {
        let side = match self.current_side() {
            Some(s) => s,
            None => return,
        };
        let pos = self.current_pos();
        if !pos.hand(side).has(kind) {
            self.message = format!("{}は持ち駒にありません", piece_kind_ja(kind));
            return;
        }
        self.focus = FocusArea::Hand;
        self.selection = Selection::HandPiece(kind);
        self.update_highlights();
        self.message = format!("{}打ち — 移動先を選んでください", piece_kind_ja(kind));
        // 駒台フォーカスに入ったらすぐにボード側でも使えるよう board に戻す
        self.focus = FocusArea::Board;
    }

    fn confirm_hand_selection(&mut self) {
        if let Selection::HandPiece(kind) = self.selection {
            self.focus = FocusArea::Board;
            self.message = format!("{}を持っています — 打ち先のマスを選んでください", piece_kind_ja(kind));
        }
    }

    // ─── 解決 ────────────────────────────────────────────────────────────────

    pub fn resolve_turn(&mut self) {
        let sente = match self.sente_action {
            Some(a) => a,
            None => return,
        };
        let gote = match self.gote_action {
            Some(a) => a,
            None => return,
        };

        let pos = self.current_pos();
        let res = resolve(&pos, sente, gote);

        self.last_resolution = build_resolution_text(&pos, sente, gote, &res.event);

        self.kifu.push(Ply { sente, gote });
        self.sente_action = None;
        self.gote_action = None;
        self.show_all_moves = false;

        // 玉の死判定
        if let Some(end) = check_king_death(&res.event) {
            let kind = match end {
                GameEnd::SenteLoses => {
                    self.last_resolution.push("→ 後手の勝ち（先手玉が取られた）".to_string());
                    GameOverKind::GoteWins(WinReason::KingDied)
                }
                GameEnd::GoteLoses => {
                    self.last_resolution.push("→ 先手の勝ち（後手玉が取られた）".to_string());
                    GameOverKind::SenteWins(WinReason::KingDied)
                }
                GameEnd::Draw => {
                    self.last_resolution.push("→ 引き分け（両玉が取られた）".to_string());
                    GameOverKind::Draw(DrawReason::BothKingDied)
                }
            };
            self.phase = Phase::GameOver(kind);
            return;
        }

        // 千日手チェック
        if check_sennichite(&self.kifu) {
            self.last_resolution.push("→ 千日手（引き分け）".to_string());
            self.phase = Phase::GameOver(GameOverKind::Draw(DrawReason::Sennichite));
            return;
        }

        // 着手不能チェック
        let next_pos = self.kifu.current();
        match check_status(&next_pos) {
            GameStatus::SenteLoses => {
                self.last_resolution.push("→ 後手の勝ち（先手着手不能）".to_string());
                self.phase = Phase::GameOver(GameOverKind::GoteWins(WinReason::Checkmate));
                return;
            }
            GameStatus::GoteLoses => {
                self.last_resolution.push("→ 先手の勝ち（後手着手不能）".to_string());
                self.phase = Phase::GameOver(GameOverKind::SenteWins(WinReason::Checkmate));
                return;
            }
            GameStatus::Draw => {
                self.last_resolution.push("→ 引き分け（両者着手不能）".to_string());
                self.phase = Phase::GameOver(GameOverKind::Draw(DrawReason::BothCheckmate));
                return;
            }
            GameStatus::Ongoing => {}
        }

        self.phase = Phase::SenteInput;
        self.cursor_file = 5;
        self.cursor_rank = 9;
        self.message = String::new();
    }

    // ─── 補助操作 ─────────────────────────────────────────────────────────────

    pub fn on_escape(&mut self) {
        match self.phase.clone() {
            Phase::PromotionChoice { .. } => {
                self.cancel_promotion();
            }
            Phase::SenteInput | Phase::GoteInput => {
                if self.selection != Selection::None {
                    self.clear_selection();
                    self.message = "選択解除".to_string();
                }
            }
            Phase::ResolveReady => {
                self.gote_action = None;
                self.phase = Phase::GoteInput;
                self.message = "後手の着手をリセット".to_string();
            }
            _ => {}
        }
    }

    pub fn undo(&mut self) {
        // ターン入力途中ならリセット
        if self.sente_action.is_some()
            || matches!(self.phase, Phase::GoteInput)
            || matches!(self.phase, Phase::ResolveReady)
        {
            self.sente_action = None;
            self.gote_action = None;
            self.phase = Phase::SenteInput;
            self.clear_selection();
            self.cursor_file = 5;
            self.cursor_rank = 9;
            self.message = "入力をリセットしました".to_string();
            return;
        }
        if matches!(self.phase, Phase::GameOver(_)) {
            // ゲームオーバー後は最後の1手を戻して続行
        }
        if self.kifu.plies.is_empty() {
            self.message = "取り消せる手がありません".to_string();
            return;
        }
        self.kifu.undo();
        self.phase = Phase::SenteInput;
        self.sente_action = None;
        self.gote_action = None;
        self.clear_selection();
        self.cursor_file = 5;
        self.cursor_rank = 9;
        self.message = "1ターン戻しました".to_string();
    }

    pub fn resign(&mut self) {
        match &self.phase {
            Phase::SenteInput => {
                self.phase = Phase::GameOver(GameOverKind::GoteWins(WinReason::Resign));
                self.message = "先手投了".to_string();
            }
            Phase::GoteInput => {
                self.phase = Phase::GameOver(GameOverKind::SenteWins(WinReason::Resign));
                self.message = "後手投了".to_string();
            }
            _ => {
                self.message = "現在投了できません".to_string();
            }
        }
    }

    pub fn save(&mut self, path: &str) {
        let content = kifu_to_string(&self.kifu);
        match std::fs::write(path, content) {
            Ok(_) => self.message = format!("棋譜を {} に保存しました", path),
            Err(e) => self.message = format!("保存エラー: {}", e),
        }
    }

    pub fn load(&mut self, path: &str) {
        match std::fs::read_to_string(path) {
            Err(e) => self.message = format!("読み込みエラー: {}", e),
            Ok(content) => match kifu_from_string(&content) {
                None => self.message = "棋譜のパースに失敗しました".to_string(),
                Some(loaded) => {
                    self.kifu = loaded;
                    self.phase = Phase::SenteInput;
                    self.sente_action = None;
                    self.gote_action = None;
                    self.clear_selection();
                    self.cursor_file = 5;
                    self.cursor_rank = 9;
                    self.last_resolution.clear();
                    self.message = format!("{} を読み込みました", path);
                }
            },
        }
    }

    pub fn toggle_sfen(&mut self) {
        if self.show_sfen {
            self.show_sfen = false;
        } else {
            let pos = self.current_pos();
            self.sfen_text = position_to_sfen(&pos);
            self.show_sfen = true;
        }
    }

    pub fn toggle_all_moves(&mut self) {
        if self.show_all_moves {
            self.show_all_moves = false;
            self.all_moves_text.clear();
        } else {
            let pos = self.current_pos();
            let side = self.current_side().unwrap_or(Side::Sente);
            let actions = legal_actions(&pos, side);
            self.all_moves_text = actions.iter().map(|a| a.to_usi()).collect();
            let label = match side {
                Side::Sente => "先手",
                Side::Gote => "後手",
            };
            self.message = format!("{} の合法手: {}手", label, self.all_moves_text.len());
            self.show_all_moves = true;
        }
    }

    pub fn new_game(&mut self) {
        *self = App::new();
    }
}

// ─── ヘルパー関数 ──────────────────────────────────────────────────────────────

pub fn piece_kind_ja(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Pawn => "歩",
        PieceKind::Lance => "香",
        PieceKind::Knight => "桂",
        PieceKind::Silver => "銀",
        PieceKind::Gold => "金",
        PieceKind::Bishop => "角",
        PieceKind::Rook => "飛",
        PieceKind::King => "玉",
        PieceKind::ProPawn => "と",
        PieceKind::ProLance => "杏",
        PieceKind::ProKnight => "圭",
        PieceKind::ProSilver => "全",
        PieceKind::Horse => "馬",
        PieceKind::Dragon => "龍",
    }
}

fn build_resolution_text(
    pos: &Position,
    sente: Action,
    gote: Action,
    event: &ResolutionEvent,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("先手: {}  後手: {}", sente.to_usi(), gote.to_usi()));

    let sente_from = sente.from_sq();
    let gote_from  = gote.from_sq();

    let sente_used_king = sente_from
        .and_then(|sq| pos.board.get(sq))
        .map_or(false, |p| p.kind == PieceKind::King);
    let gote_used_king = gote_from
        .and_then(|sq| pos.board.get(sq))
        .map_or(false, |p| p.kind == PieceKind::King);

    // 戦国無双の真の救済判定:
    //   玉が駒を取った「かつ」相手がその玉を狙っていた場合のみ★
    //   留まっている駒を取っただけなら通常取得。
    //   スワップ救済: 後手の行先 == 玉の元居たマス
    //   同一マス救済: 後手の行先 == 玉の移動先（打ち込み含む）
    let sente_musou = sente_used_king && (
        sente_from.map_or(false, |f| gote.to_sq() == f)
        || gote.to_sq() == sente.to_sq()
    );
    let gote_musou = gote_used_king && (
        gote_from.map_or(false, |f| sente.to_sq() == f)
        || sente.to_sq() == gote.to_sq()
    );

    match event {
        ResolutionEvent::Normal {
            sente_capture,
            gote_capture,
        } => {
            if let Some(k) = sente_capture {
                if sente_musou {
                    lines.push(format!(
                        "★戦国無双: 先手玉が{}を斬り返した",
                        piece_kind_ja(k.unpromoted())
                    ));
                } else {
                    lines.push(format!("先手が{}を取得", piece_kind_ja(k.unpromoted())));
                }
            }
            if let Some(k) = gote_capture {
                if gote_musou {
                    lines.push(format!(
                        "★戦国無双: 後手玉が{}を斬り返した",
                        piece_kind_ja(k.unpromoted())
                    ));
                } else {
                    lines.push(format!("後手が{}を取得", piece_kind_ja(k.unpromoted())));
                }
            }
            if sente_capture.is_none() && gote_capture.is_none() {
                lines.push("取得なし（逃げ・空き移動）".to_string());
            }
        }
        ResolutionEvent::Clash {
            sente_piece,
            gote_piece,
        } => {
            lines.push(format!(
                "相討ち: 先手の{}と後手の{}が交換",
                piece_kind_ja(sente_piece.unpromoted()),
                piece_kind_ja(gote_piece.unpromoted())
            ));
        }
        ResolutionEvent::SenteDied => lines.push("先手玉が取られた！".to_string()),
        ResolutionEvent::GoteDied => lines.push("後手玉が取られた！".to_string()),
        ResolutionEvent::BothDied => lines.push("両玉が同時に取られた！".to_string()),
    }
    lines
}

// ─── ゲームオーバー文言 ────────────────────────────────────────────────────────

pub fn game_over_text(kind: &GameOverKind) -> &'static str {
    match kind {
        GameOverKind::SenteWins(WinReason::KingDied) => "先手の勝ち（後手玉が取られた）",
        GameOverKind::SenteWins(WinReason::Resign) => "先手の勝ち（後手投了）",
        GameOverKind::SenteWins(WinReason::Checkmate) => "先手の勝ち（後手着手不能）",
        GameOverKind::GoteWins(WinReason::KingDied) => "後手の勝ち（先手玉が取られた）",
        GameOverKind::GoteWins(WinReason::Resign) => "後手の勝ち（先手投了）",
        GameOverKind::GoteWins(WinReason::Checkmate) => "後手の勝ち（先手着手不能）",
        GameOverKind::Draw(DrawReason::BothKingDied) => "引き分け（両玉が取られた）",
        GameOverKind::Draw(DrawReason::BothCheckmate) => "引き分け（両者着手不能）",
        GameOverKind::Draw(DrawReason::Sennichite) => "引き分け（千日手）",
    }
}
