use wasm_bindgen::prelude::*;

/// 着手の日本語棋譜表記を返す。
///
/// - usi:        着手の USI 表記（例: "7g7f", "7g7f+", "P*5e", "resign"）
/// - side:       着手した陣営（"sente" | "gote"）
/// - legal_json: engine-wasm の legal_actions() が返す JSON 配列
///               例: `["7g7f","6g6f","P*5e"]`
/// - sfen:       着手前の局面 SFEN
///
/// 成功: "７六歩"、"５八金右" 等の日本語文字列
/// 失敗（不正入力）: 空文字列
#[wasm_bindgen]
pub fn ja_notation(usi: &str, side: &str, legal_json: &str, sfen: &str) -> String {
    let action = match engine::types::Action::from_usi(usi) {
        Some(a) => a,
        None    => return String::new(),
    };
    let s = match side {
        "gote" => engine::types::Side::Gote,
        _      => engine::types::Side::Sente,
    };
    let pos = match engine::serialize::sfen_to_position(sfen) {
        Some(p) => p,
        None    => return String::new(),
    };
    let legal_actions = parse_legal_json(legal_json);

    notation::ja_notation(&action, s, &legal_actions, &pos)
}

/// `["7g7f","6g6f","P*5e"]` 形式の JSON 配列を Action の Vec にパースする。
/// serde を使わず engine-wasm の出力形式に特化したシンプルな実装。
fn parse_legal_json(json: &str) -> Vec<engine::types::Action> {
    let trimmed = json.trim();
    if trimmed == "[]" || trimmed.is_empty() {
        return vec![];
    }
    let inner = trimmed
        .strip_prefix('[')
        .unwrap_or(trimmed)
        .strip_suffix(']')
        .unwrap_or(trimmed);
    inner
        .split(',')
        .filter_map(|token| {
            let usi = token.trim().trim_matches('"');
            engine::types::Action::from_usi(usi)
        })
        .collect()
}
