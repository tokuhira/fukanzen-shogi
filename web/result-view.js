// 結果・終局の日本語表示文字列（presentation / i18n）。純粋（board.js 分割 第一段a）。

const RESULT_KIND_JA = {
  mate:       '詰み',
  king_death: '玉が取られた',
  swap_draw:  '両玉相討ち',
  sennichite: '千日手',
  resign:     '投了',
  unfinished: '未完',
  other:      'その他',
};
const OUTCOME_JA = {
  sente_wins: '先手の勝ち',
  gote_wins:  '後手の勝ち',
  draw:       '引き分け',
  none:       '',
};

export function formatResult(result) {
  const kindJa = RESULT_KIND_JA[result.kind] || result.kind;
  if (result.outcome === 'none') return kindJa;
  const outcomeJa = OUTCOME_JA[result.outcome] || result.outcome;
  return `${outcomeJa}（${kindJa}）`;
}

// maxTurns はモジュールグローバルを掴まず引数で受ける（純粋化）。
export function terminalMessageJa(kind, outcome, maxTurns) {
  if (kind === 'mate') {
    if (outcome === 'gote_wins')  return '後手の勝ち（先手が着手不能）';
    if (outcome === 'sente_wins') return '先手の勝ち（後手が着手不能）';
    if (outcome === 'draw')       return '引き分け（両者着手不能）';
  }
  if (kind === 'king_death') {
    if (outcome === 'gote_wins')  return '後手の勝ち（先手玉が取られた）';
    if (outcome === 'sente_wins') return '先手の勝ち（後手玉が取られた）';
  }
  if (kind === 'swap_draw'  && outcome === 'draw') return '引き分け（両玉相討ち）';
  if (kind === 'sennichite' && outcome === 'draw') return '引き分け（千日手）';
  if (kind === 'max_turns'  && outcome === 'draw') return `引き分け（最長手数・${maxTurns}組手）`;
  return null;
}
