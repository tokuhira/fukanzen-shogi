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

/// アーカイブ書式 v1（または旧 sfen 始まり）のテキストを解釈して対局データを返す。
/// `build_archive` の対。
///
/// 成功: `{"ok":true,"initial_sfen":"...","plies":[{"s":"7g7f","g":"3c3d"},...],
///        "meta":{"rule":"0.5","protocol":2,"app":"0.8.0","sente":null,"gote":null,
///                "result":{"kind":"mate","outcome":"gote_wins"}}}`
/// 失敗: `{"ok":false,"error":"<理由>"}`
#[wasm_bindgen]
pub fn parse_archive(text: &str) -> String {
    let (kifu, meta) = match engine::archive::archive_to_kifu(text) {
        Some(v) => v,
        None => return r#"{"ok":false,"error":"parse_failed"}"#.to_string(),
    };

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

fn escape_json(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', r#"\""#)
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
}
