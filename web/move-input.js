// 着手組み立ての純粋計算。パース済み合法手配列（parseUsi 済み）を受け、状態・DOM・Wasm に
// 触れず値を返す。board.js 分割 第二段b（案A）。
// move = { usi, isDrop, kind?, from?:[f,r], to:[f,r], promote }

// 盤上マス (file,rank) から動ける手だけに絞る。
export function movesFromSquare(moves, file, rank) {
  return moves.filter(m => !m.isDrop && m.from[0] === file && m.from[1] === rank);
}

// 持ち駒 kind の打ちだけに絞る（kind は大文字化して比較）。
export function dropsOfKind(moves, kind) {
  return moves.filter(m => m.isDrop && m.kind === kind.toUpperCase());
}

// 手の集合を「到達点 → options」のマップに畳む。options[i] = { usi, promote }。
export function buildTargetMap(moves) {
  const map = new Map();
  for (const m of moves) {
    const key = `${m.to[0]},${m.to[1]}`;
    if (!map.has(key)) map.set(key, { options: [] });
    map.get(key).options.push({ usi: m.usi, promote: m.promote });
  }
  return map;
}

// 到達点クリックから次アクションを決める（成り不成の判定）。状態は変えず、意図だけ返す。
//   到達点でない        → { kind: 'deselect' }
//   成・不成が両立       → { kind: 'promptPromotion', options, toSquare:[f,r] }
//   一方のみ            → { kind: 'confirm', usi }
export function resolveTarget(targetMap, file, rank) {
  const key = `${file},${rank}`;
  if (!targetMap.has(key)) return { kind: 'deselect' };
  const { options } = targetMap.get(key);
  const hasPromote   = options.some(o =>  o.promote);
  const hasNoPromote = options.some(o => !o.promote);
  if (hasPromote && hasNoPromote) return { kind: 'promptPromotion', options, toSquare: [file, rank] };
  return { kind: 'confirm', usi: options[0].usi };
}
