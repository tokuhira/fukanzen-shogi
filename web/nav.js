// 局面ナビゲーションの純粋な状態遷移。view = 状態の必要部分、action = 'prev' | 'next'。
// 次状態の patch（変化分）を返す。ナビ不可・状態不変なら null（呼び出し側は再描画のみ）。
// board.js 分割 第三段b-2。DOM・Wasm・可変状態に非依存。
//   view = { phase:'position'|'reveal', cursor:number, pliesLen:number,
//            onlineMode:boolean, onlineGameOver:boolean }
export function navReduce(view, action) {
  // オンライン対局中（終局前）はナビ不可。
  if (view.onlineMode && !view.onlineGameOver) return null;

  if (action === 'prev') {
    if (view.phase === 'reveal') return { phase: 'position' };
    if (view.phase === 'position' && view.cursor > 0) return { cursor: view.cursor - 1, phase: 'reveal' };
    return null;
  }
  if (action === 'next') {
    if (view.phase === 'position' && view.cursor < view.pliesLen) return { phase: 'reveal' };
    if (view.phase === 'reveal') return { cursor: view.cursor + 1, phase: 'position' };
    return null;
  }
  return null;
}
