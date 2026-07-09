// 棋譜コアの純粋な遷移。record = { sfens, events, plies } を受けて新しい record を返す
// （不変・破壊的変更なし）。Wasm（resolvePly）と usiToText は呼び出し側から注入する
// （本モジュールは Wasm に直接依存しない）。board.js 分割 第二段a。
//
// resolvePly(sfen, sUsi, gUsi) -> { ok, sfen, event } | { ok:false, error }
// usiToText(usi, sfen, side)   -> 日本語棋譜テキスト（side: 'sente' | 'gote'）

export function emptyRecord(initialSfen) {
  return { sfens: [initialSfen], events: [], plies: [] };
}

// record の末尾に一組手を適用した新しい record を返す。sText/gText が渡されなければ導出。
export function appendTurn(record, sUsi, gUsi, resolvePly, usiToText, sText, gText) {
  const preSfen = record.sfens.at(-1);
  const r = resolvePly(preSfen, sUsi, gUsi);
  if (!r.ok) throw new Error(r.error);
  return {
    sfens:  [...record.sfens, r.sfen],
    events: [...record.events, r.event],
    plies:  [...record.plies, {
      sUsi, gUsi,
      sText: sText ?? usiToText(sUsi, preSfen, 'sente'),
      gText: gText ?? usiToText(gUsi, preSfen, 'gote'),
    }],
  };
}

// record を組手数 n までに切り詰めた新しい record を返す（sfens は n+1 本＝局面列）。
export function truncateTo(record, n) {
  return {
    sfens:  record.sfens.slice(0, n + 1),
    events: record.events.slice(0, n),
    plies:  record.plies.slice(0, n),
  };
}

// plies 列から record を組み立て直す（loadPlies の芯）。各 ply は {sUsi,gUsi,sText?,gText?}。
export function buildFromPlies(initialSfen, plies, resolvePly, usiToText) {
  let record = emptyRecord(initialSfen);
  for (const ply of plies) {
    record = appendTurn(record, ply.sUsi, ply.gUsi, resolvePly, usiToText, ply.sText, ply.gText);
  }
  return record;
}
