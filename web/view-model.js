// render() の「state → 表示値」の導出を担う純粋モジュール（board.js view 純粋化 View-1）。
// wasm を呼ばない——盤面から導く終局メッセージ（gameOverMsg）は呼び出し側が注入する。

import { formatResult } from './result-view.js';
import { inputOverlay, revealOverlay } from './board-view.js'; // 純粋（wasm 非依存）

// 事象ラベル（表示定数）。board.js から移設。
const EVENT_LABEL = {
  clash:      '相討ち',
  sente_died: '先手玉が取られた',
  gote_died:  '後手玉が取られた',
  both_died:  '両玉相討ち',
};

export function watchPhaseText(state, gameOver) {
  if (state.watchStatusText === 'connecting') return '観戦: 接続中…';
  if (state.watchStatusText === 'error')      return '観戦: 接続エラーが発生しました';
  if (state.watchStatusText === 'closed')     return '観戦: 接続が切れました';

  // 投了など盤面から導けない終局は result で判断する（player_disconnected は
  // 対局終了時の意図した WS 切断でも届くため、既に終局済みなら「再接続待ち」
  // という誤解を招く表示にしない）。
  const concluded = !!(state.loadedMeta?.result && state.loadedMeta.result.kind !== 'unfinished');
  if (state.watchStatusText === 'player_disconnected' && !concluded) {
    return '観戦: プレイヤーが切断中です（再接続を待っています）';
  }
  if (concluded && state.cursor === state.plies.length) return formatResult(state.loadedMeta.result);
  if (gameOver) return gameOver;
  if (state.plies.length === 0) return '観戦中（開始を待っています）';
  if (state.cursor === state.plies.length) return '観戦中（最新）';
  return `観戦中（第${state.cursor}組手）`;
}

export function onlinePhaseText(state, gameOver) {
  if (state.onlineGameOver) {
    if (gameOver || state.cursor === state.plies.length) return state.onlineEndMsg || gameOver || '終局';
    if (state.cursor === 0) return '初期局面';
    return `第${state.cursor}組手後`;
  }
  if (state.onlineWaiting)   return state.onlineWaitingMsg;
  if (state.onlineCommitted) return '着手確定 — 相手の着手を待っています';
  if (state.onlineSide === 'gote') return state.selectedFrom ? '後手の手を選択中' : '後手の手を選んでください';
  return state.selectedFrom ? '先手の手を選択中' : '先手の手を選んでください';
}

// 読み込んだアーカイブの版タプル・結果を鑑賞表示する。版不一致なら注意を返す。
export function archiveInfoText(state) {
  if (!state.loadedMeta) return { text: '', mismatch: false };

  const versionLine = state.loadedMeta.app
    ? `ルール ${state.loadedMeta.rule} / プロトコル ${state.loadedMeta.protocol} / v${state.loadedMeta.app}`
    : `ルール ${state.loadedMeta.rule} / プロトコル ${state.loadedMeta.protocol}`;
  const resultLine = formatResult(state.loadedMeta.result);

  const mismatch = !!(state.versionTuple && state.loadedMeta.rule !== state.versionTuple.rule);
  if (!mismatch) {
    return { text: `${versionLine} — ${resultLine}`, mismatch: false };
  }
  const warning =
    `この棋譜はルール ${state.loadedMeta.rule} で指されました。現在の再生エンジンはルール ${state.versionTuple.rule} です。` +
    `再生結果が当時と異なる可能性があります。`;
  return { text: `${versionLine} — ${resultLine} ／ ${warning}`, mismatch: true };
}

/**
 * ラベル系の表示値を state（＋盤面から導く終局メッセージ gameOverMsg）から純粋に組む。
 * wasm 非依存（gameOverMsg は呼び出し側が注入する）。
 */
export function labelView(state, gameOverMsg) {
  let moveText = '', phaseText = '', eventText = '';

  if (state.phase === 'reveal') {
    const ply = state.plies[state.cursor];
    moveText = `${ply.sText}　${ply.gText}`;
    phaseText = '同時開示';
    const evKey = state.events[state.cursor];
    eventText = (evKey && evKey !== 'normal') ? `（${EVENT_LABEL[evKey] || evKey}）` : '';
  } else {
    const bothReady = !!(state.pendingSente && state.pendingGote);
    if (state.watchMode) {
      phaseText = watchPhaseText(state, gameOverMsg);
    } else if (state.onlineMode) {
      phaseText = onlinePhaseText(state, gameOverMsg);
      if (!state.onlineGameOver && state.onlineCommitted) {
        moveText = state.onlineSide === 'sente' ? (state.pendingSente?.text || '') : (state.pendingGote?.text || '');
      }
    } else if (bothReady) {
      moveText = `${state.pendingSente.text}　${state.pendingGote.text}`;
      phaseText = '解決してください';
    } else if (state.pendingSente) {
      moveText = state.pendingSente.text;
      phaseText = '後手の手を選択中';
    } else if (state.inputStep === 'gote') {
      phaseText = '後手の手を選択中';
    } else if (state.inputStep === 'sente' || state.selectedFrom) {
      phaseText = '先手の手を選択中';
    } else if (gameOverMsg) {
      phaseText = gameOverMsg;
    } else if (state.cursor === 0) {
      phaseText = '初期局面';
    } else {
      phaseText = `第${state.cursor}組手後`;
    }
  }

  const archiveInfo = archiveInfoText(state);
  const total = state.plies.length * 2 + 1;
  const step = state.cursor * 2 + (state.phase === 'reveal' ? 1 : 0) + 1;

  return { phaseText, moveText, eventText, archiveInfo, step, total };
}

/**
 * ボタンの表示状態を state（＋終局メッセージ gameOverMsg）から純粋に組む。
 * wasm 非依存（gameOverMsg 注入）。ロジックは現行 render() のボタン節を一字一句移す。
 */
export function buttonView(state, gameOverMsg) {
  const bothReady = !!(state.pendingSente && state.pendingGote);
  const hasInput  = !!(state.inputStep || state.selectedFrom || state.pendingSente || state.pendingGote);
  const atStart   = state.cursor === 0 && state.phase === 'position';
  const canForward = state.phase === 'reveal' || (state.phase === 'position' && state.cursor < state.plies.length);

  let next, prev;
  if (state.watchMode) {
    next = { text: '次 →', disabled: !canForward };
    prev = { disabled: atStart };
  } else if (state.onlineMode) {
    if (state.onlineGameOver) {
      next = { text: '次 →', disabled: !canForward };
      prev = { disabled: atStart };
    } else {
      next = { text: '次 →', disabled: true };
      prev = { disabled: true };
    }
  } else {
    next = {
      text: bothReady ? '解決 →' : '次 →',
      disabled: !(bothReady || state.phase === 'reveal' ||
                  (state.phase === 'position' && !hasInput && state.cursor < state.plies.length)),
    };
    prev = { disabled: state.cursor === 0 && state.phase === 'position' && !hasInput && !state.promotionPending };
  }

  const resign = {
    visible: state.onlineMode && !state.onlineGameOver,
    disabled: state.onlineCommitted || state.onlineWaiting,
  };
  const isOver = state.onlineMode ? state.onlineGameOver : !!gameOverMsg;
  const save = { highlight: isOver };
  const startButtonsDisabled = state.watchMode;   // btn-online, btn-load
  const leaveWatchHidden = !state.watchMode;

  return { next, prev, resign, save, startButtonsDisabled, leaveWatchHidden };
}

/** overlay（reveal→開示 overlay／入力中→入力 overlay／それ以外→null）。純粋。 */
export function overlay(state) {
  if (state.phase === 'reveal') {
    return revealOverlay(state.plies[state.cursor]);
  }
  const hasInput = !!(state.inputStep || state.selectedFrom || state.pendingSente || state.pendingGote);
  return hasInput
    ? inputOverlay({ selectedFrom: state.selectedFrom, inputStep: state.inputStep, legalTargets: state.legalTargets })
    : null;
}

/** 盤の SVG カーソルがポインタ（操作可能）か。純粋。 */
export function cursorInteractive(state, gameOverMsg) {
  return state.phase === 'position'
    && !gameOverMsg
    && !state.watchMode
    && !(state.onlineMode && state.onlineCommitted);
}

/** 描画に必要な表示値を一つの束に合成する（pos・gameOverMsg は wasm 依存なので呼び出し側が用意）。純粋。 */
export function viewModel(state, gameOverMsg) {
  return {
    ...labelView(state, gameOverMsg),         // phaseText, moveText, eventText, archiveInfo, step, total
    buttons: buttonView(state, gameOverMsg),  // next, prev, resign, save, startButtonsDisabled, leaveWatchHidden
    overlay: overlay(state),
    cursorInteractive: cursorInteractive(state, gameOverMsg),
  };
}
