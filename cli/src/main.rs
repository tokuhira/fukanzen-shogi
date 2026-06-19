/// 不完全将棋 検証用 CLI（第一段階）
///
/// 一人が両陣営の着手を入力して一局を最後まで進める検証モード。
/// 秘匿性なし・単一プロセス。ルールエンジンの正しさを人手で確かめるためのツール。
use engine::board::Position;
use engine::kifu::Kifu;
use engine::movegen::legal_actions;
use engine::resolve::{resolve, ResolutionEvent};
use engine::serialize::{kifu_from_string, kifu_to_string, ply_to_string, position_to_sfen};
use engine::terminate::{check_king_death, check_status, GameEnd, GameStatus};
use engine::types::{Action, PieceKind, Ply, Side};
use std::io::{self, BufRead, Write};

fn main() {
    println!("不完全将棋 検証用 CLI — 第一段階");
    println!("表示コマンド: :board  :kifu  :moves [s|g]  :sfen");
    println!("進行コマンド: :quit  :resign [s|g]  :undo");
    println!("補助コマンド: :load <path>  :save <path>");
    println!();

    let stdin = io::stdin();
    let mut kifu = Kifu::new(Position::initial());
    let mut game_result: Option<GameResult> = None;

    loop {
        let pos = kifu.current();
        print_board(&pos);

        // 着手選択前の終了判定（確定的詰み）
        let status = check_status(&pos);
        match status {
            GameStatus::SenteLoses => {
                println!("先手（手前）が着手不能 — 確定的詰み。後手の勝ち。");
                game_result = Some(GameResult::GoteWins(WinReason::Checkmate));
                break;
            }
            GameStatus::GoteLoses => {
                println!("後手（上手）が着手不能 — 確定的詰み。先手の勝ち。");
                game_result = Some(GameResult::SenteWins(WinReason::Checkmate));
                break;
            }
            GameStatus::Draw => {
                println!("両者が着手不能 — 引き分け。");
                game_result = Some(GameResult::Draw(DrawReason::BothCheckmate));
                break;
            }
            GameStatus::Ongoing => {}
        }

        // 先手の決断を収集
        let sente_decision = match input_action(&mut stdin.lock(), &pos, Side::Sente, &mut kifu) {
            InputResult::Decided(d) => d,
            InputResult::Quit => break,
            InputResult::Reload => continue,
        };

        // 後手の決断を収集
        let gote_decision = match input_action(&mut stdin.lock(), &pos, Side::Gote, &mut kifu) {
            InputResult::Decided(d) => d,
            InputResult::Quit => break,
            InputResult::Reload => continue,
        };

        // 両決断を照合（仕様書 §5.3/5.4）
        let (sente_act, gote_act) = match (sente_decision, gote_decision) {
            (Decision::Resign, Decision::Resign) => {
                println!("両者が同時に投了。引き分け。");
                game_result = Some(GameResult::Draw(DrawReason::BothResign));
                break;
            }
            (Decision::Resign, Decision::Move(_)) => {
                println!("先手が投了。後手の勝ち。");
                game_result = Some(GameResult::GoteWins(WinReason::Resign));
                break;
            }
            (Decision::Move(_), Decision::Resign) => {
                println!("後手が投了。先手の勝ち。");
                game_result = Some(GameResult::SenteWins(WinReason::Resign));
                break;
            }
            (Decision::Move(s), Decision::Move(g)) => (s, g),
        };

        // 解決
        let res = resolve(&pos, sente_act, gote_act);
        print_resolution(&res.event, sente_act, gote_act);

        // 棋譜に追加
        kifu.push(Ply {
            sente: sente_act,
            gote: gote_act,
        });

        // 玉の死の判定
        if let Some(end) = check_king_death(&res.event) {
            match end {
                GameEnd::SenteLoses => {
                    println!("先手玉が取られた。後手の勝ち。");
                    game_result = Some(GameResult::GoteWins(WinReason::KingDied));
                }
                GameEnd::GoteLoses => {
                    println!("後手玉が取られた。先手の勝ち。");
                    game_result = Some(GameResult::SenteWins(WinReason::KingDied));
                }
                GameEnd::Draw => {
                    println!("両玉が同時に取られた。引き分け。");
                    game_result = Some(GameResult::Draw(DrawReason::BothKingDied));
                }
            }
            break;
        }

        // 千日手チェック
        if engine::terminate::check_sennichite(&kifu) {
            // TODO: 仕様書 §7（未確定）— 指し直しか引き分けかは要再検討。暫定引き分け。
            println!("千日手成立（同一局面4回）。暫定引き分け。[要再検討 §7]");
            game_result = Some(GameResult::Draw(DrawReason::Sennichite));
            break;
        }
    }

    println!("─── 最終局面 ───");
    print_board(&kifu.current());
    print_kifu(&kifu, game_result.as_ref());
    println!("対局終了。お疲れ様でした。");
}

/// プレイヤーが一ターンに下す決断（着手または投了）
enum Decision {
    Move(Action),
    Resign,
}

enum InputResult {
    Decided(Decision),
    Quit,
    Reload,
}

enum WinReason {
    Resign,
    KingDied,
    Checkmate,
}

enum DrawReason {
    BothResign,
    BothKingDied,
    BothCheckmate,
    Sennichite,
}

enum GameResult {
    SenteWins(WinReason),
    GoteWins(WinReason),
    Draw(DrawReason),
}

fn input_action(
    reader: &mut impl BufRead,
    pos: &Position,
    side: Side,
    kifu: &mut Kifu,
) -> InputResult {
    let side_label = match side {
        Side::Sente => "先手",
        Side::Gote => "後手",
    };
    let legal = legal_actions(pos, side);

    loop {
        print!("{} の着手を入力 (USI例 7g7f, P*5e): ", side_label);
        io::stdout().flush().unwrap();

        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return InputResult::Quit;
        }
        let input = line.trim();

        if input.starts_with(':') {
            match handle_command(input, pos, side, kifu) {
                Some(r) => return r,
                None => continue,
            }
        }

        match Action::from_usi(input) {
            None => {
                println!("  入力形式が不正です。USI 形式で入力してください（例: 7g7f, P*5e）");
            }
            Some(action) => {
                if legal.contains(&action) {
                    return InputResult::Decided(Decision::Move(action));
                } else {
                    // 非合法の理由を診断して表示
                    println!("  {} の合法手ではありません。", input);
                    diagnose_illegal(pos, side, action);
                    println!("  (:moves s または :moves g で合法手一覧を表示)");
                }
            }
        }
    }
}

/// "s"/"sente" → Sente、"g"/"gote" → Gote、それ以外は None（エラー）
fn parse_side_arg(arg: &str) -> Option<Side> {
    match arg {
        "s" | "sente" => Some(Side::Sente),
        "g" | "gote" => Some(Side::Gote),
        _ => None,
    }
}

fn handle_command(
    input: &str,
    pos: &Position,
    side: Side,
    kifu: &mut Kifu,
) -> Option<InputResult> {
    let parts: Vec<&str> = input.splitn(3, ' ').collect();
    match parts[0] {
        ":moves" => {
            let target = if parts.len() >= 2 {
                match parse_side_arg(parts[1]) {
                    Some(s) => s,
                    None => {
                        println!("  使い方: :moves [s|g]  （s=先手、g=後手）");
                        return None;
                    }
                }
            } else {
                side
            };
            let actions = legal_actions(pos, target);
            let label = match target { Side::Sente => "先手", Side::Gote => "後手" };
            println!("  {} の合法手 ({} 手):", label, actions.len());
            for a in &actions {
                print!("  {}", a.to_usi());
            }
            println!();
            None
        }
        ":board" => {
            print_board(pos);
            None
        }
        ":undo" => {
            if kifu.plies.is_empty() {
                println!("  取り消せる手がありません。");
                None
            } else {
                kifu.undo();
                println!("  1組手戻りました。");
                Some(InputResult::Reload)
            }
        }
        ":save" => {
            if parts.len() < 2 {
                println!("  使い方: :save <ファイルパス>");
                return None;
            }
            let path = parts[1];
            let content = kifu_to_string(kifu);
            match std::fs::write(path, content) {
                Ok(_) => println!("  棋譜を {} に保存しました。", path),
                Err(e) => println!("  保存エラー: {}", e),
            }
            None
        }
        ":load" => {
            if parts.len() < 2 {
                println!("  使い方: :load <ファイルパス>");
                return None;
            }
            let path = parts[1];
            match std::fs::read_to_string(path) {
                Err(e) => println!("  読み込みエラー: {}", e),
                Ok(content) => match kifu_from_string(&content) {
                    None => println!("  棋譜のパースに失敗しました。"),
                    Some(loaded) => {
                        *kifu = loaded;
                        println!("  棋譜を {} から読み込みました。", path);
                        return Some(InputResult::Reload);
                    }
                },
            }
            None
        }
        ":resign" => {
            if parts.len() >= 2 {
                match parse_side_arg(parts[1]) {
                    None => {
                        println!("  使い方: :resign [s|g]  （s=先手、g=後手）");
                        return None;
                    }
                    Some(s) if s != side => {
                        let side_label = match side { Side::Sente => "先手", Side::Gote => "後手" };
                        let other_label = match s { Side::Sente => "先手", Side::Gote => "後手" };
                        println!("  現在は{}の入力フェーズです。{}の投了はその入力フェーズで行ってください。",
                            side_label, other_label);
                        return None;
                    }
                    Some(_) => {} // 現在の陣営と一致、続行
                }
            }
            Some(InputResult::Decided(Decision::Resign))
        }
        ":quit" | ":exit" => Some(InputResult::Quit),
        ":sfen" => {
            println!("  {}", position_to_sfen(pos));
            None
        }
        ":kifu" => {
            print_kifu(kifu, None);
            None
        }
        _ => {
            println!("  不明なコマンド: {}", input);
            None
        }
    }
}

fn print_kifu(kifu: &Kifu, result: Option<&GameResult>) {
    if kifu.plies.is_empty() {
        println!("  着手なし（初期局面）");
    } else {
        println!("  棋譜（{}組手）:", kifu.plies.len());
        let start = kifu.initial_position.move_number;
        for (i, ply) in kifu.plies.iter().enumerate() {
            println!("  {}", ply_to_string(start + i as u32, ply));
        }
    }
    if let Some(r) = result {
        println!("  {}", game_result_line(r, kifu.plies.len()));
    }
}

fn game_result_line(result: &GameResult, n: usize) -> String {
    match result {
        GameResult::SenteWins(reason) => {
            let r = match reason {
                WinReason::Resign   => "後手投了",
                WinReason::KingDied => "後手玉死",
                WinReason::Checkmate => "後手着手不能",
            };
            format!("まで{}組手で先手の勝ち（{}）", n, r)
        }
        GameResult::GoteWins(reason) => {
            let r = match reason {
                WinReason::Resign   => "先手投了",
                WinReason::KingDied => "先手玉死",
                WinReason::Checkmate => "先手着手不能",
            };
            format!("まで{}組手で後手の勝ち（{}）", n, r)
        }
        GameResult::Draw(reason) => {
            let r = match reason {
                DrawReason::BothResign    => "両者投了",
                DrawReason::BothKingDied  => "両玉死",
                DrawReason::BothCheckmate => "両者着手不能",
                DrawReason::Sennichite    => "千日手",
            };
            format!("まで{}組手で引き分け（{}）", n, r)
        }
    }
}

fn diagnose_illegal(pos: &Position, side: Side, action: Action) {
    match action {
        Action::Drop { kind: PieceKind::Pawn, to } => {
            let file = to.file();
            let has_own_pawn = pos.board.iter().any(|(sq, p)| {
                p.side == side && p.kind == PieceKind::Pawn && sq.file() == file
            });
            if has_own_pawn {
                println!("  理由: 二歩（同じ筋にすでに歩があります）");
                return;
            }
        }
        Action::Move { from, to, promote } => {
            if let Some(p) = pos.board.get(from) {
                if p.side != side {
                    println!("  理由: 自分の駒ではありません");
                    return;
                }
            } else {
                println!("  理由: 移動元に駒がありません");
                return;
            }
            // 成り忘れ: promote:false で非合法だが promote:true なら合法 → 成りが必須
            if !promote {
                let with_promote = Action::Move { from, to, promote: true };
                if legal_actions(pos, side).contains(&with_promote) {
                    println!(
                        "  理由: この移動は成りが必須です（{} と入力してください）",
                        with_promote.to_usi()
                    );
                    return;
                }
            }
        }
        _ => {}
    }
    println!("  理由: その手は合法手の範囲外です");
}

fn print_board(pos: &Position) {
    println!();
    println!("  組手: {}", pos.move_number - 1);

    // 後手の持ち駒
    print!("後手持駒: ");
    print_hand(&pos.hand_gote);
    println!();

    // 盤面（後手視点で段1が上）
    println!("  ９ ８ ７ ６ ５ ４ ３ ２ １");
    println!(" +--+--+--+--+--+--+--+--+--+");
    for rank in 1u8..=9 {
        let rank_char = (b'a' + rank - 1) as char;
        print!("{}|", rank_char);
        for file in (1u8..=9).rev() {
            let sq = engine::types::Square::new(file, rank);
            match pos.board.get(sq) {
                None => print!(" . "),
                Some(p) => {
                    let c = piece_display_char(p);
                    print!("{}", c);
                }
            }
        }
        println!("|");
    }
    println!(" +--+--+--+--+--+--+--+--+--+");

    // 先手の持ち駒
    print!("先手持駒: ");
    print_hand(&pos.hand_sente);
    println!("\n");
}

fn print_hand(hand: &engine::board::Hand) {
    let mut any = false;
    for (kind, cnt) in hand.iter() {
        let c = piece_kind_ja(kind);
        if cnt > 1 {
            print!("{}{} ", c, cnt);
        } else {
            print!("{} ", c);
        }
        any = true;
    }
    if !any {
        print!("なし");
    }
}

fn piece_display_char(p: engine::types::Piece) -> String {
    let name = match p.kind {
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
    };
    match p.side {
        Side::Sente => format!(" {}", name),   // 先手: 通常表示
        Side::Gote => format!("v{}", name),    // 後手: v プレフィクス
    }
}

fn piece_kind_ja(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Pawn     => "歩",
        PieceKind::Lance    => "香",
        PieceKind::Knight   => "桂",
        PieceKind::Silver   => "銀",
        PieceKind::Gold     => "金",
        PieceKind::Bishop   => "角",
        PieceKind::Rook     => "飛",
        PieceKind::King     => "玉",
        PieceKind::ProPawn  => "と",
        PieceKind::ProLance => "杏",
        PieceKind::ProKnight => "圭",
        PieceKind::ProSilver => "全",
        PieceKind::Horse    => "馬",
        PieceKind::Dragon   => "龍",
    }
}

fn print_resolution(event: &ResolutionEvent, sente: Action, gote: Action) {
    println!("  --- 解決結果 ---");
    println!("  先手: {} | 後手: {}", sente.to_usi(), gote.to_usi());
    match event {
        ResolutionEvent::Normal { sente_capture, gote_capture } => {
            if let Some(k) = sente_capture {
                println!("  先手が {} を取得", piece_kind_ja(*k));
            }
            if let Some(k) = gote_capture {
                println!("  後手が {} を取得", piece_kind_ja(*k));
            }
            if sente_capture.is_none() && gote_capture.is_none() {
                println!("  取得なし（空きマスへの移動、または逃げた駒）");
            }
        }
        ResolutionEvent::Clash { sente_piece, gote_piece } => {
            println!(
                "  相討ち: 先手の {} と後手の {} が交換",
                piece_kind_ja(*sente_piece),
                piece_kind_ja(*gote_piece)
            );
        }
        ResolutionEvent::SenteDied => println!("  先手玉が取られた！"),
        ResolutionEvent::GoteDied => println!("  後手玉が取られた！"),
        ResolutionEvent::BothDied => println!("  両玉が同時に取られた！"),
    }
    println!("  ----------------");
}
