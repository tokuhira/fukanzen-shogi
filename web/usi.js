// USI 文字列の純粋な解釈。DOM・Wasm・可変状態に非依存（board.js 分割 第一段a）。

const RANK_CHAR = 'abcdefghi';

export function charToRank(c) { return RANK_CHAR.indexOf(c) + 1; }

export function parseUsi(usi) {
  if (usi[1] === '*') {
    return { usi, isDrop: true, kind: usi[0], to: [parseInt(usi[2]), charToRank(usi[3])], promote: false };
  }
  return {
    usi,
    isDrop:  false,
    from:    [parseInt(usi[0]), charToRank(usi[1])],
    to:      [parseInt(usi[2]), charToRank(usi[3])],
    promote: usi.length === 5,
  };
}
