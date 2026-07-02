use wasm_bindgen::prelude::*;

/// 両着手を解決して次局面と発生事象を返す。
///
/// - sfen: 現局面の SFEN 文字列
/// - sente_usi: 先手の USI 着手（例: "7g7f", "P*8f", "8h3c+"）
/// - gote_usi:  後手の USI 着手
///
/// 成功: `{"ok":true,"sfen":"<次局面>","event":"normal|clash|sente_died|gote_died|both_died"}`
/// 失敗: `{"ok":false,"error":"<理由>"}`
#[wasm_bindgen]
pub fn resolve_ply(sfen: &str, sente_usi: &str, gote_usi: &str) -> String {
    let pos = match engine::serialize::sfen_to_position(sfen) {
        Some(p) => p,
        None => return format!(r#"{{"ok":false,"error":"bad sfen: {}"}}"#, escape_json(sfen)),
    };
    let sente = match engine::types::Action::from_usi(sente_usi) {
        Some(a) => a,
        None => return format!(r#"{{"ok":false,"error":"bad sente_usi: {}"}}"#, escape_json(sente_usi)),
    };
    let gote = match engine::types::Action::from_usi(gote_usi) {
        Some(a) => a,
        None => return format!(r#"{{"ok":false,"error":"bad gote_usi: {}"}}"#, escape_json(gote_usi)),
    };

    let resolution = engine::resolve::resolve(&pos, sente, gote);
    let next_sfen = engine::serialize::position_to_sfen(&resolution.next);

    let event = match &resolution.event {
        engine::resolve::ResolutionEvent::Normal { .. } => "normal",
        engine::resolve::ResolutionEvent::Clash { .. }  => "clash",
        engine::resolve::ResolutionEvent::SenteDied     => "sente_died",
        engine::resolve::ResolutionEvent::GoteDied      => "gote_died",
        engine::resolve::ResolutionEvent::BothDied      => "both_died",
    };

    format!(r#"{{"ok":true,"sfen":"{}","event":"{}"}}"#, escape_json(&next_sfen), event)
}

/// 指定局面のゲーム状態を返す（着手選択前の確定詰みチェック）。
///
/// 返値: "ongoing" | "sente_loses" | "gote_loses" | "draw" | "error"
#[wasm_bindgen]
pub fn game_status(sfen: &str) -> String {
    let pos = match engine::serialize::sfen_to_position(sfen) {
        Some(p) => p,
        None => return "error".to_string(),
    };
    match engine::terminate::check_status(&pos) {
        engine::terminate::GameStatus::Ongoing    => "ongoing".to_string(),
        engine::terminate::GameStatus::SenteLoses => "sente_loses".to_string(),
        engine::terminate::GameStatus::GoteLoses  => "gote_loses".to_string(),
        engine::terminate::GameStatus::Draw       => "draw".to_string(),
    }
}

/// 指定局面・陣営の合法手を USI 文字列の JSON 配列として返す。
///
/// - sfen: 局面の SFEN 文字列
/// - side: "sente" | "gote"
///
/// 返値: `["7g7f","P*5e",...]`（空なら `[]`）
#[wasm_bindgen]
pub fn legal_actions(sfen: &str, side: &str) -> String {
    let pos = match engine::serialize::sfen_to_position(sfen) {
        Some(p) => p,
        None => return "[]".to_string(),
    };
    let s = match side {
        "sente" => engine::types::Side::Sente,
        "gote"  => engine::types::Side::Gote,
        _ => return "[]".to_string(),
    };
    let actions = engine::movegen::legal_actions(&pos, s);
    let usis: Vec<String> = actions.iter()
        .map(|a| format!("\"{}\"", a.to_usi()))
        .collect();
    format!("[{}]", usis.join(","))
}

/// 対局データを版タプル付きアーカイブ書式 v1 のテキストへ変換する。
///
/// request_json:
/// `{"initial_sfen":"...","plies":[{"s":"7g7f","g":"3c3d"},...],
///   "rule":"0.5","protocol":2,"app":"0.8.0","sente":null,"gote":null,
///   "result":{"kind":"mate","outcome":"gote_wins"}}`
///
/// 成功: アーカイブ本文の文字列
/// 失敗: `"ERROR: <理由>"`
#[wasm_bindgen]
pub fn build_archive(request_json: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(request_json) {
        Ok(v) => v,
        Err(_) => return "ERROR: invalid_json".to_string(),
    };

    let initial_sfen = match v["initial_sfen"].as_str() {
        Some(s) => s,
        None => return "ERROR: missing initial_sfen".to_string(),
    };
    let initial = match engine::serialize::sfen_to_position(initial_sfen) {
        Some(p) => p,
        None => return "ERROR: bad initial_sfen".to_string(),
    };
    let mut kifu = engine::kifu::Kifu::new(initial);

    let plies = match v["plies"].as_array() {
        Some(a) => a,
        None => return "ERROR: missing plies".to_string(),
    };
    for p in plies {
        let s_usi = match p["s"].as_str() {
            Some(s) => s,
            None => return "ERROR: missing ply.s".to_string(),
        };
        let g_usi = match p["g"].as_str() {
            Some(s) => s,
            None => return "ERROR: missing ply.g".to_string(),
        };
        let sente = match engine::types::Action::from_usi(s_usi) {
            Some(a) => a,
            None => return format!("ERROR: bad ply.s: {}", s_usi),
        };
        let gote = match engine::types::Action::from_usi(g_usi) {
            Some(a) => a,
            None => return format!("ERROR: bad ply.g: {}", g_usi),
        };
        kifu.push(engine::types::Ply { sente, gote });
    }

    let rule_str = match v["rule"].as_str() {
        Some(s) => s,
        None => return "ERROR: missing rule".to_string(),
    };
    let rule = match rule_str.split_once('.') {
        Some((a, b)) => match (a.parse::<u32>(), b.parse::<u32>()) {
            (Ok(a), Ok(b)) => (a, b),
            _ => return "ERROR: bad rule".to_string(),
        },
        None => return "ERROR: bad rule".to_string(),
    };
    let protocol = match v["protocol"].as_u64() {
        Some(p) => p as u32,
        None => return "ERROR: missing protocol".to_string(),
    };
    let app = v["app"].as_str().map(|s| s.to_string());
    let sente = v["sente"].as_str().map(|s| s.to_string());
    let gote = v["gote"].as_str().map(|s| s.to_string());

    let kind = match v["result"]["kind"].as_str().and_then(engine::archive::ResultKind::from_str) {
        Some(k) => k,
        None => return "ERROR: bad result.kind".to_string(),
    };
    let outcome = match v["result"]["outcome"].as_str().and_then(engine::archive::Outcome::from_str) {
        Some(o) => o,
        None => return "ERROR: bad result.outcome".to_string(),
    };

    let meta = engine::archive::ArchiveMeta {
        rule,
        protocol,
        app,
        sente,
        gote,
        result: engine::archive::ArchiveResult { kind, outcome },
    };

    engine::archive::kifu_to_archive(&kifu, &meta)
}

/// ルール v0.6 の最長手数（組手）。`engine::terminate::MAX_TURNS` が単一の値であり、
/// アーカイブ読込の安全網（`parse_archive`）もここから参照する（ハードコードの
/// 重複を持たない）。web 側もこの getter から値を取得し、JS 側に定数を複製しない。
#[wasm_bindgen]
pub fn max_turns() -> usize {
    engine::terminate::MAX_TURNS
}

/// アーカイブ書式 v1（または旧 sfen 始まり）のテキストを解釈して対局データを返す。
/// `build_archive` の対。
///
/// 成功: `{"ok":true,"initial_sfen":"...","plies":[{"s":"7g7f","g":"3c3d"},...],
///        "meta":{"rule":"0.5","protocol":2,"app":"0.8.0","sente":null,"gote":null,
///                "result":{"kind":"mate","outcome":"gote_wins"}}}`
/// 失敗: `{"ok":false,"error":"<理由>"}`（着手数超過時は `"too_many_plies"`）
#[wasm_bindgen]
pub fn parse_archive(text: &str) -> String {
    let (kifu, meta) = match engine::archive::archive_to_kifu(text) {
        Some(v) => v,
        None => return r#"{"ok":false,"error":"parse_failed"}"#.to_string(),
    };

    if kifu.plies.len() > engine::terminate::MAX_TURNS {
        return r#"{"ok":false,"error":"too_many_plies"}"#.to_string();
    }

    let initial_sfen = engine::serialize::position_to_sfen(&kifu.initial_position);
    let plies: Vec<String> = kifu
        .plies
        .iter()
        .map(|p| format!(
            r#"{{"s":"{}","g":"{}"}}"#,
            escape_json(&p.sente.to_usi()),
            escape_json(&p.gote.to_usi())
        ))
        .collect();

    let app_json = match &meta.app {
        Some(s) => format!(r#""{}""#, escape_json(s)),
        None => "null".to_string(),
    };
    let sente_json = match &meta.sente {
        Some(s) => format!(r#""{}""#, escape_json(s)),
        None => "null".to_string(),
    };
    let gote_json = match &meta.gote {
        Some(s) => format!(r#""{}""#, escape_json(s)),
        None => "null".to_string(),
    };

    format!(
        r#"{{"ok":true,"initial_sfen":"{}","plies":[{}],"meta":{{"rule":"{}.{}","protocol":{},"app":{},"sente":{},"gote":{},"result":{{"kind":"{}","outcome":"{}"}}}}}}"#,
        escape_json(&initial_sfen),
        plies.join(","),
        meta.rule.0,
        meta.rule.1,
        meta.protocol,
        app_json,
        sente_json,
        gote_json,
        meta.result.kind.to_str(),
        meta.result.outcome.to_str(),
    )
}

/// 棋譜（初期局面＋着手列）から盤上の終局を評価する（投了を除く。ルール v0.6 §5.8）。
/// `build_archive` と同じ流儀で initial_sfen＋plies から Kifu を構成し、
/// `engine::terminate::evaluate` を呼んで、結果を archive の語彙
/// （`ResultKind`/`Outcome`）に対応づけて返す。
///
/// request_json: `{"initial_sfen":"...","plies":[{"s":"7g7f","g":"3c3d"}, ...]}`
///
/// 成功: `{"status":"ongoing"}` または
///       `{"status":"terminal","kind":"mate","outcome":"gote_wins"}`
/// 失敗: `{"status":"error","error":"<理由>"}`
#[wasm_bindgen]
pub fn evaluate_terminal(request_json: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(request_json) {
        Ok(v) => v,
        Err(_) => return r#"{"status":"error","error":"invalid_json"}"#.to_string(),
    };

    let initial_sfen = match v["initial_sfen"].as_str() {
        Some(s) => s,
        None => return r#"{"status":"error","error":"missing initial_sfen"}"#.to_string(),
    };
    let initial = match engine::serialize::sfen_to_position(initial_sfen) {
        Some(p) => p,
        None => return r#"{"status":"error","error":"bad initial_sfen"}"#.to_string(),
    };
    let mut kifu = engine::kifu::Kifu::new(initial);

    let plies = match v["plies"].as_array() {
        Some(a) => a,
        None => return r#"{"status":"error","error":"missing plies"}"#.to_string(),
    };
    for p in plies {
        let s_usi = match p["s"].as_str() {
            Some(s) => s,
            None => return r#"{"status":"error","error":"missing ply.s"}"#.to_string(),
        };
        let g_usi = match p["g"].as_str() {
            Some(s) => s,
            None => return r#"{"status":"error","error":"missing ply.g"}"#.to_string(),
        };
        let sente = match engine::types::Action::from_usi(s_usi) {
            Some(a) => a,
            None => return format!(r#"{{"status":"error","error":"bad ply.s: {}"}}"#, escape_json(s_usi)),
        };
        let gote = match engine::types::Action::from_usi(g_usi) {
            Some(a) => a,
            None => return format!(r#"{{"status":"error","error":"bad ply.g: {}"}}"#, escape_json(g_usi)),
        };
        kifu.push(engine::types::Ply { sente, gote });
    }

    use engine::archive::{Outcome, ResultKind};
    use engine::terminate::{DrawKind, LossKind, Terminal};
    use engine::types::Side;

    let (kind, outcome) = match engine::terminate::evaluate(&kifu) {
        Terminal::Ongoing => return r#"{"status":"ongoing"}"#.to_string(),
        Terminal::Loss { loser: Side::Sente, kind: LossKind::Mate } => (ResultKind::Mate, Outcome::GoteWins),
        Terminal::Loss { loser: Side::Gote, kind: LossKind::Mate } => (ResultKind::Mate, Outcome::SenteWins),
        Terminal::Loss { loser: Side::Sente, kind: LossKind::KingDeath } => {
            (ResultKind::KingDeath, Outcome::GoteWins)
        }
        Terminal::Loss { loser: Side::Gote, kind: LossKind::KingDeath } => {
            (ResultKind::KingDeath, Outcome::SenteWins)
        }
        Terminal::Draw { kind: DrawKind::MutualMate } => (ResultKind::Mate, Outcome::Draw),
        Terminal::Draw { kind: DrawKind::BothKingsDied } => (ResultKind::SwapDraw, Outcome::Draw),
        Terminal::Draw { kind: DrawKind::Sennichite } => (ResultKind::Sennichite, Outcome::Draw),
        Terminal::Draw { kind: DrawKind::MaxTurns } => (ResultKind::MaxTurns, Outcome::Draw),
    };

    format!(
        r#"{{"status":"terminal","kind":"{}","outcome":"{}"}}"#,
        kind.to_str(),
        outcome.to_str()
    )
}

/// JSON 文字列リテラルとして安全な形にエスケープする。
///
/// `\` と `"` に加え、JSON 仕様上リテラル埋め込みが禁止されている制御文字
/// （U+0000〜U+001F）も `\uXXXX` へエスケープする。ここを漏らすと、外部から
/// 読み込んだアーカイブのヘッダ自由記述欄（app/sente/gote）に生の制御文字が
/// 混入した場合に、手組み JSON の構文が壊れて JSON.parse が例外を投げる
/// （実際に確認済み: タブ混入で "Bad control character in string literal"）。
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '"'  => out.push_str("\\\""),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> String {
        r#"{"initial_sfen":"lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            "plies":[{"s":"7g7f","g":"3c3d"},{"s":"2g2f","g":"8c8d"}],
            "rule":"0.5","protocol":2,"app":"0.8.1",
            "sente":null,"gote":null,
            "result":{"kind":"mate","outcome":"gote_wins"}}"#
            .to_string()
    }

    #[test]
    fn build_then_parse_round_trip() {
        let archive = build_archive(&sample_request());
        assert!(!archive.starts_with("ERROR"), "build_archive failed: {}", archive);

        let parsed_json = parse_archive(&archive);
        let v: serde_json::Value = serde_json::from_str(&parsed_json).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(
            v["initial_sfen"],
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1"
        );
        assert_eq!(v["plies"].as_array().unwrap().len(), 2);
        assert_eq!(v["plies"][0]["s"], "7g7f");
        assert_eq!(v["plies"][0]["g"], "3c3d");
        assert_eq!(v["plies"][1]["s"], "2g2f");
        assert_eq!(v["plies"][1]["g"], "8c8d");
        assert_eq!(v["meta"]["rule"], "0.5");
        assert_eq!(v["meta"]["protocol"], 2);
        assert_eq!(v["meta"]["app"], "0.8.1");
        assert_eq!(v["meta"]["sente"], serde_json::Value::Null);
        assert_eq!(v["meta"]["gote"], serde_json::Value::Null);
        assert_eq!(v["meta"]["result"]["kind"], "mate");
        assert_eq!(v["meta"]["result"]["outcome"], "gote_wins");
    }

    #[test]
    fn parse_old_bare_kifu() {
        let old = "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1\n1: 7g7f | 3c3d";
        let parsed_json = parse_archive(old);
        let v: serde_json::Value = serde_json::from_str(&parsed_json).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["plies"].as_array().unwrap().len(), 1);
        assert_eq!(v["meta"]["result"]["kind"], "unfinished");
        assert_eq!(v["meta"]["result"]["outcome"], "none");
    }

    #[test]
    fn parse_broken_input() {
        let parsed_json = parse_archive("this is not an archive");
        let v: serde_json::Value = serde_json::from_str(&parsed_json).unwrap();
        assert_eq!(v["ok"], false);
    }

    #[test]
    fn parse_empty_input() {
        let parsed_json = parse_archive("");
        let v: serde_json::Value = serde_json::from_str(&parsed_json).unwrap();
        assert_eq!(v["ok"], false);
    }

    fn request_with_n_plies(n: usize) -> String {
        let plies_json = std::iter::repeat(r#"{"s":"7g7f","g":"3c3d"}"#)
            .take(n)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"initial_sfen":"lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
                "plies":[{}],
                "rule":"0.5","protocol":2,"app":"0.8.1",
                "sente":null,"gote":null,
                "result":{{"kind":"unfinished","outcome":"none"}}}}"#,
            plies_json
        )
    }

    #[test]
    fn parse_accepts_exactly_max_plies() {
        let archive = build_archive(&request_with_n_plies(engine::terminate::MAX_TURNS));
        assert!(!archive.starts_with("ERROR"), "build_archive failed: {}", archive);
        let v: serde_json::Value = serde_json::from_str(&parse_archive(&archive)).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["plies"].as_array().unwrap().len(), engine::terminate::MAX_TURNS);
    }

    #[test]
    fn parse_rejects_too_many_plies() {
        let archive = build_archive(&request_with_n_plies(engine::terminate::MAX_TURNS + 1));
        assert!(!archive.starts_with("ERROR"), "build_archive failed: {}", archive);
        let v: serde_json::Value = serde_json::from_str(&parse_archive(&archive)).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], "too_many_plies");
    }

    #[test]
    fn escape_json_escapes_control_characters() {
        // タブ混入の app 欄を持つアーカイブが、壊れた JSON にならず正しく
        // 往復することを確認する（修正前は手組み JSON が構文エラーになっていた）。
        let archive = concat!(
            "fukanzen-shogi-archive 1\n",
            "rule 0.5\n",
            "protocol 2\n",
            "app foo\tbar\n",
            "sente -\n",
            "gote -\n",
            "result unfinished none\n",
            "sfen lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1\n",
        );
        let parsed_json = parse_archive(archive);
        let v: serde_json::Value = serde_json::from_str(&parsed_json)
            .expect("parse_archive の出力が不正な JSON になっている");
        assert_eq!(v["ok"], true);
        assert_eq!(v["meta"]["app"], "foo\tbar");
    }

    #[test]
    fn max_turns_getter_matches_engine_constant() {
        assert_eq!(max_turns(), engine::terminate::MAX_TURNS);
        assert_eq!(max_turns(), 500);
    }

    fn terminal_request(initial_sfen: &str, plies_json: &str) -> String {
        format!(
            r#"{{"initial_sfen":"{}","plies":[{}]}}"#,
            initial_sfen, plies_json
        )
    }

    #[test]
    fn evaluate_terminal_ongoing() {
        let sfen = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";
        let v: serde_json::Value =
            serde_json::from_str(&evaluate_terminal(&terminal_request(sfen, ""))).unwrap();
        assert_eq!(v["status"], "ongoing");
    }

    /// 500組手まで一度も内容が重複しない、玉2枚だけの初期局面＋着手列を
    /// (initial_sfen, plies_json) として組み立てる。terminate.rs の
    /// no_repeat_kifu と同じ CRT の考え方（周期36・35は互いに素）を USI 経由で再現し、
    /// 千日手を誤検出せず最長手数だけを試験できるようにする。
    fn no_repeat_request(n_plies: usize) -> String {
        use engine::board::{Board, Hand, Position};
        use engine::types::{Action, Piece, PieceKind, Side, Square};

        fn half_squares(rank_lo: u8, rank_hi: u8) -> Vec<Square> {
            let mut v = Vec::new();
            for rank in rank_lo..=rank_hi {
                for file in 1..=9u8 {
                    v.push(Square::new(file, rank));
                }
            }
            v
        }

        let sente_squares = half_squares(1, 4); // 36マス
        let gote_squares: Vec<Square> = half_squares(6, 9).into_iter().take(35).collect(); // 35マス

        let mut board = Board::empty();
        board.set(sente_squares[0], Some(Piece::new(PieceKind::King, Side::Sente)));
        board.set(gote_squares[0], Some(Piece::new(PieceKind::King, Side::Gote)));
        let initial_sfen = engine::serialize::position_to_sfen(&Position {
            board,
            hand_sente: Hand::empty(),
            hand_gote: Hand::empty(),
            move_number: 1,
        });

        let mut sente_at = 0usize;
        let mut gote_at = 0usize;
        let mut plies = Vec::new();
        for i in 0..n_plies {
            let next_sente = (i + 1) % sente_squares.len();
            let next_gote = (i + 1) % gote_squares.len();
            let s_usi = Action::Move { from: sente_squares[sente_at], to: sente_squares[next_sente], promote: false }.to_usi();
            let g_usi = Action::Move { from: gote_squares[gote_at], to: gote_squares[next_gote], promote: false }.to_usi();
            plies.push(format!(r#"{{"s":"{}","g":"{}"}}"#, s_usi, g_usi));
            sente_at = next_sente;
            gote_at = next_gote;
        }

        terminal_request(&initial_sfen, &plies.join(","))
    }

    #[test]
    fn evaluate_terminal_max_turns() {
        let request = no_repeat_request(engine::terminate::MAX_TURNS);
        let v: serde_json::Value = serde_json::from_str(&evaluate_terminal(&request)).unwrap();
        assert_eq!(v["status"], "terminal");
        assert_eq!(v["kind"], "max_turns");
        assert_eq!(v["outcome"], "draw");
    }

    #[test]
    fn evaluate_terminal_matches_build_archive_plies() {
        // build_archive → parse_archive の整合（同じ着手列が正しく往復する）ことを
        // no_repeat_request の initial_sfen＋plies を使って別途確認する。
        let n = 3;
        let request = no_repeat_request(n);
        let req_v: serde_json::Value = serde_json::from_str(&request).unwrap();
        let archive_request = format!(
            r#"{{"initial_sfen":{},"plies":{},"rule":"0.6","protocol":2,"app":"test","sente":null,"gote":null,"result":{{"kind":"unfinished","outcome":"none"}}}}"#,
            req_v["initial_sfen"], req_v["plies"]
        );
        let archive = build_archive(&archive_request);
        assert!(!archive.starts_with("ERROR"), "build_archive failed: {}", archive);
        let reparsed: serde_json::Value = serde_json::from_str(&parse_archive(&archive)).unwrap();
        assert_eq!(reparsed["plies"].as_array().unwrap().len(), n);
    }

    #[test]
    fn evaluate_terminal_bad_input() {
        let v: serde_json::Value =
            serde_json::from_str(&evaluate_terminal("not json")).unwrap();
        assert_eq!(v["status"], "error");
    }
}
