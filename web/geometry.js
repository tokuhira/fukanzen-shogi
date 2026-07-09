// 盤の座標系・字形・数え字。描画（board-view.js）とヒットテスト（board.js）が
// 共に読む共有基盤。純粋データ＋純粋ヘルパ（board.js 分割 第一段a）。

export const CELL  = 38;
export const BX    = 6;
export const BY    = 58;
export const BW    = CELL * 9;        // 342
export const BH    = CELL * 9;        // 342
export const SVG_W = BX + BW + 30;    // 378
export const SVG_H = BY + BH + 50;    // 450
export const PFS   = 22;
export const LFS   = 11;

export const KANJI = {
  P:'歩', L:'香', N:'桂', S:'銀', G:'金', B:'角', R:'飛', K:'玉',
  '+P':'と', '+L':'杏', '+N':'圭', '+S':'全', '+B':'馬', '+R':'龍',
};
export const HAND_ORDER = ['R','B','G','S','N','L','P'];
export const RANK_JA    = ['一','二','三','四','五','六','七','八','九'];

export function countStr(n) {
  if (n <= 1) return '';
  return n <= 9 ? RANK_JA[n - 1] : String(n);
}
