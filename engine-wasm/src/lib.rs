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

fn escape_json(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', r#"\""#)
}
