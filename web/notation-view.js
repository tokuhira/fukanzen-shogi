// 着手（USI）を日本語棋譜テキストへ。Wasm 関数（legalActions・jaNotation）は
// 呼び出し側から注入する（本モジュールは Wasm に直接依存しない）。board.js 分割 第一段b。

// legalActions(sfen, side) -> legal_json, jaNotation(usi, side, legal_json, sfen) -> text
export function usiToText(usi, sfen, side, legalActions, jaNotation) {
  const prefix    = side === "sente" ? "☗" : "☖";
  const legalJson = legalActions(sfen, side);
  return `${prefix}${jaNotation(usi, side, legalJson, sfen)}`;
}
