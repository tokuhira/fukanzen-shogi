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
