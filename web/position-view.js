// engine-wasm の position_view が返す JSON view を、盤（board.js）の消費者形
// （{board:Map, handS, handG}）へ組み替える純粋アダプタ。副作用なし・Wasm 非依存
// （盤面解釈そのものは engine::serialize::sfen_to_position が単一の正本——
// board.js 分割 第〇段）。DOM に触れないため、Wasm を経由せず単体でテストできる。

export function positionViewToState(view) {
  const board = new Map();
  for (const sq of view.board) {
    board.set(`${sq.file},${sq.rank}`, { kind: sq.kind, side: sq.side });
  }
  return { board, handS: view.hand_s || {}, handG: view.hand_g || {} };
}
