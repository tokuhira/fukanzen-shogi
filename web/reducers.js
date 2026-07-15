// 状態遷移の純粋 reduce 群（種類1）。値を受け patch（変化分）を返す。
// DOM・Wasm・可変状態に非依存。board.js 分割 第三段b-3。
// （局面ナビの navReduce は nav.js に別置。将来ここへ集約する余地はあるが本書では移さない。）

// オンライン関連の状態を初期値へ戻す patch。対局終了・退出・新規対局で使う。
export function resetOnlineReduce() {
  return {
    onlineMode: false,
    onlineSide: null,
    onlineGameOver: false,
    onlineEndMsg: '',
    onlineCommitted: false,
    onlineWaiting: false,
    onlineWaitingMsg: '',
    resultOverride: null,
    recordInviteAsked: false,
    recordStatusText: '',
    archivedLink: null,
    _pendingRecordDisconnect: false,
  };
}

// ホットシート（同一端末で両者指す）モードの確定後の状態遷移 patch。
// side==='sente' なら後手入力へ進む。text は呼び出し側が usiToText で作って渡す。
//   pending = { usi, text }
export function hotseatConfirmReduce(side, pending) {
  if (side === 'sente') {
    return { pendingSente: pending, inputStep: 'gote', selectedFrom: null, legalTargets: null, promotionPending: null };
  }
  return { pendingGote: pending, selectedFrom: null, legalTargets: null, promotionPending: null };
}

// オンライン対局で組手が揃ったときの「投了判断」を純粋に行う（ルール 5.3/5.4）。
// 投了なら勝敗メッセージ・outcome・resultOverride を返す。投了でなければ
// {kind:'live'}（合法性検証・通常 append は呼び出し側＝殻が担う）。
// wasm 非依存（合法性・表示テキストは殻で扱う）。
export function turnCompleteDecision(senteUsi, goteUsi, onlineSide) {
  const sResign = senteUsi === 'resign';
  const gResign = goteUsi  === 'resign';
  if (!sResign && !gResign) return { kind: 'live' };

  let msg, outcome;
  if (sResign && gResign) {
    msg = '引き分け（両者投了）';
    outcome = 'draw';
  } else if (sResign) {
    msg = onlineSide === 'sente' ? '投了しました（後手の勝ち）' : '相手が投了しました（先手の勝ち）';
    outcome = 'gote_wins';
  } else {
    msg = onlineSide === 'gote'  ? '投了しました（先手の勝ち）' : '相手が投了しました（後手の勝ち）';
    outcome = 'sente_wins';
  }
  return { kind: 'resign', msg, outcome, resultOverride: { kind: 'resign', outcome } };
}

// 版タプルと結果から loadedMeta を組む。純粋（board.js の _metaToLoadedMeta を移設。
// onInit・onMeta で共有）。
export function metaToLoadedMeta(version, result) {
  if (!version) return null;
  return {
    rule: version.rule, protocol: version.protocol, app: version.app,
    sente: null, gote: null,
    result: result ?? { kind: 'unfinished', outcome: 'none' },
  };
}

// アーカイブ id からリンク情報を組む。id が無ければ null。純粋（archiveUrl 注入）。
export function archivedLinkFor(id, archiveUrl) {
  return id ? { id, url: archiveUrl(id) } : null;
}
