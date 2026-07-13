import init, {
  resolve_ply,
  legal_actions as wasmLegalActions,
  build_archive as wasmBuildArchive,
  parse_archive as wasmParseArchive,
  evaluate_terminal as wasmEvaluateTerminal,
  max_turns as wasmMaxTurns,
  position_view as wasmPositionView,
} from './wasm/engine_wasm.js';

import { positionViewToState } from './position-view.js';

import initNotation, {
  ja_notation as wasmJaNotation,
} from './notation-wasm/notation_wasm.js';

import initProtocol, {
  version_tuple as wasmVersionTuple,
} from './protocol-wasm/protocol_wasm.js';

import {
  connectOnline, disconnectOnline, commitMoveOnline, getMySide, abortOnline,
  reconnectOnline, hasReconnectableSession, debugState,
  sendSpectateMeta, sendSpectateResult, connectSpectate, disconnectSpectate,
  sendRecordInvite, sendRecordAccept, sendRecordDecline, sendRecordTestimony,
  isRecording, archiveUrl,
} from './online.js';

import { parseUsi } from './usi.js';
import { terminalMessageJa } from './result-view.js';
import {
  CELL, BX, BY, BW, BH, SVG_W, SVG_H, PFS, KANJI, HAND_ORDER, countStr,
} from './geometry.js';
import { renderSvg, inputOverlay, revealOverlay } from './board-view.js';
import { usiToText as usiToTextPure } from './notation-view.js';
import { emptyRecord, appendTurn, truncateTo, buildFromPlies } from './game-record.js';
import { movesFromSquare, dropsOfKind, buildTargetMap, resolveTarget } from './move-input.js';
import { navReduce } from './nav.js';
import { resetOnlineReduce, hotseatConfirmReduce } from './reducers.js';
import { labelView } from './view-model.js';

// ── Constants ─────────────────────────────────────────────────────────────────

const INITIAL_SFEN = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

// 読込を受け付けるアーカイブテキストの最大バイト数（安全弁）。
// 500組手の正当なアーカイブは概算 13〜14KB（着手行 500×24B＋ヘッダ約1.2KB）。
// 巨大な悪意あるファイルでブラウザを固まらせないための、十分な安全マージン。
const MAX_ARCHIVE_BYTES = 512 * 1024;

// 実 Wasm 関数を綴じ込んだ board.js ローカルの呼び出し口（既存の呼び出し形を保つ）。
function usiToText(usi, sfen, side) {
  return usiToTextPure(usi, sfen, side, wasmLegalActions, wasmJaNotation);
}

// 純粋な game-record へ実 Wasm を渡す注入口（resolve_ply は JSON パースして渡す）。
const resolvePly = (sfen, sUsi, gUsi) => JSON.parse(resolve_ply(sfen, sUsi, gUsi));

// 両陣営の着手が現局面で合法かを検証する（resolve_ply へ渡す前の安全弁）。
// resolve_ply（engine::resolve()）は両着手が既に合法であることを前提とする契約
// なので、相手の reveal（拘束性・盤面ハッシュしか検証されていない）をそのまま
// 渡すと wasm パニックになりうる（不正な相手からの攻撃面。TUI 側 online.rs の
// turn_actions_are_legal と対称）。
function turnActionsAreLegal(sfen, sUsi, gUsi) {
  const sLegal = JSON.parse(wasmLegalActions(sfen, 'sente'));
  const gLegal = JSON.parse(wasmLegalActions(sfen, 'gote'));
  return sLegal.includes(sUsi) && gLegal.includes(gUsi);
}

// 棋譜（アーカイブ）の全 ply が、その時点の局面で合法な着手かを検証する
// （読込の安全弁）。resolve_ply は両着手が既に合法であることを前提とする契約
// なので、検証前に loadPlies（内部で resolve_ply を呼ぶ）へ渡すと非合法な
// 棋譜（改竄・共有元の悪意）でパニックしうる——ここで先に弾く。
// このバレ棋譜書式は投了を表現しないので、投了を含む ply も不正として扱う
// （TUI 側 app.rs::kifu_all_plies_legal と対称）。
function pliesAreAllLegal(initialSfen, plies) {
  let sfen = initialSfen;
  for (const { sUsi, gUsi } of plies) {
    if (sUsi === 'resign' || gUsi === 'resign') return false;
    if (!turnActionsAreLegal(sfen, sUsi, gUsi)) return false;
    sfen = resolvePly(sfen, sUsi, gUsi).sfen;
  }
  return true;
}

// ── SFEN parser ───────────────────────────────────────────────────────────────
//
// 盤面解釈そのもの（SFEN が意味する盤面・持ち駒）は engine-wasm の position_view
// （engine::serialize::sfen_to_position が単一の正本）へ委譲する。JSON view を
// 従来の消費者形（Map と持ち駒オブジェクト）へ組み替える純粋アダプタは
// position-view.js（board.js 分割 第〇段）。

function parseSfen(sfen) {
  return positionViewToState(JSON.parse(wasmPositionView(sfen)));
}

// ── Kifu state ────────────────────────────────────────────────────────────────

const state = {
  // 棋譜コア
  cursor: 0,
  sfens: [INITIAL_SFEN],   // sfens[i] = position entering turn i
  events: [],              // events[i] = event string from resolving plies[i]
  plies: [],               // plies[i] = { sUsi, gUsi, sText, gText }（第三段b-1でkifuから吸収）
  phase: 'position',       // 'position' | 'reveal'

  // 入力
  inputStep: null,         // null | 'sente' | 'gote'
  pendingSente: null,      // null | { usi, text }
  pendingGote: null,
  selectedFrom: null,      // null | { board:[f,r] } | { hand:kind }
  legalTargets: null,      // null | Map<"f,r", { options:[{usi,promote}] }>
  promotionPending: null,  // null | { options, toSquare }

  // キャッシュ
  legalCache: { sfen: null, sente: null, gote: null },
  gameOverCache: { cursor: -1, msg: null },

  // メタ / 結果
  versionTuple: null,      // { rule, protocol, app } — init() 完了後にキャッシュ
  resultOverride: null,    // { kind, outcome } | null — 投了など盤面から導出できない結果
  loadedMeta: null,        // 読み込んだアーカイブの ArchiveMeta（鑑賞表示・版不一致判定用）
  maxTurns: null,          // ルール v0.6 の最長手数（組手）。init() 完了後に engine-wasm から取得

  // オンライン
  onlineMode: false,
  onlineSide: null,        // 'sente' | 'gote'
  onlineCommitted: false,
  onlineGameOver: false,
  onlineEndMsg: '',
  onlineWaiting: false,
  onlineWaitingMsg: '',

  // 観戦
  watchMode: false,
  watchStatusText: '',
  spectateToken: null,     // 対局時に受け取った観戦リンク用トークン（プレイヤー側）

  // 記録係（記録係二段目）
  recordInviteAsked: false,        // このゲームで招待の可否を既に尋ねたか（二重prompt防止）
  recordStatusText: '',            // 記録係の状態表示用テキスト（最小 surface。§5）
  archivedLink: null,              // { id, url } 直近の archived 通知（GET /archive/:id へのリンク）
  _pendingRecordDisconnect: false, // 証言送信後、綴じ結果を受け取るまで切断を待っているか
};

// 状態更新の唯一の経路。patch を state へ浅くマージし、一度だけ再描画する。
// （tui の「App を更新 → terminal.draw」の分離に相似。描画は更新に従属する。）
// 注意: b-1 では reducer 化はしない——単純な浅いマージ。意味のある遷移の整理は b-2。
function update(patch) {
  Object.assign(state, patch);
  render();
}

// ── Kifu management ───────────────────────────────────────────────────────────
//
// 棋譜コアの遷移（値の計算）は game-record.js の純粋関数に委譲する。状態変数
// （sfens/events/cursor/phase/plies）は参照が広いためここに据え置き、setRecord/
// currentRecord が状態と純粋層の橋渡しをする（board.js 分割 第二段a）。

function setRecord(record) {
  state.sfens  = record.sfens;
  state.events = record.events;
  state.plies  = record.plies;
}
function currentRecord() {
  return { sfens: state.sfens, events: state.events, plies: state.plies };
}

function loadPlies(plies, initialSfen = INITIAL_SFEN) {
  setRecord(buildFromPlies(initialSfen, plies, resolvePly, usiToText));
  state.cursor = 0;
  state.phase  = 'position';
  resetInput();
  state.gameOverCache  = { cursor: -1, msg: null };
  state.resultOverride = null;
}

function resetToNew() {
  setRecord(emptyRecord(INITIAL_SFEN));
  state.cursor = 0;
  state.phase  = 'position';
  resetInput();
  state.gameOverCache  = { cursor: -1, msg: null };
  state.resultOverride = null;
  state.loadedMeta     = null;
}

function branchAndAppend(sUsi, gUsi, sText, gText) {
  setRecord(appendTurn(truncateTo(currentRecord(), state.cursor), sUsi, gUsi, resolvePly, usiToText, sText, gText));
  state.gameOverCache = { cursor: -1, msg: null };
  state.phase = 'reveal';  // cursor stays — reveal shows the move just played
  resetInput();
}

// 観戦: 受信した組手を state.plies の末尾へ追記する（分岐しない・cursor は動かさない）。
// 利用者が過去局面をレビュー中でも、ライブの新着はそのまま棋譜の末尾に積まれる。
function watchAppendTurn(sUsi, gUsi) {
  try {
    setRecord(appendTurn(currentRecord(), sUsi, gUsi, resolvePly, usiToText));
  } catch (e) {
    console.error('watch: resolve_ply failed:', e.message);
    return;
  }
  state.gameOverCache = { cursor: -1, msg: null };
}

// ── Game-over detection ───────────────────────────────────────────────────────

function getGameOverMsg() {
  if (state.phase !== 'position') return null;
  if (state.cursor !== state.gameOverCache.cursor) {
    state.gameOverCache = { cursor: state.cursor, msg: computeGameOver() };
  }
  return state.gameOverCache.msg;
}

// state.plies の先頭から uptoPlies 組手までの局面を、engine::terminate::evaluate
// （ルール v0.6 §5.8 の一元評価）で判定する。盤上で導ける終局はすべてこの一箇所
// に集約し、web 側では順序を再実装しない（アーカイブ語彙 kind/outcome を返す）。
function evaluateTerminalAt(uptoPlies) {
  const request = {
    initial_sfen: state.sfens[0],
    plies: state.plies.slice(0, uptoPlies).map(p => ({ s: p.sUsi, g: p.gUsi })),
  };
  return JSON.parse(wasmEvaluateTerminal(JSON.stringify(request)));
}

function computeGameOver() {
  const term = evaluateTerminalAt(state.cursor);
  if (term.status !== 'terminal') return null;
  return terminalMessageJa(term.kind, term.outcome, state.maxTurns);
}

// ── Archive ────────────────────────────────────────────────────────────────────

// 対局全体（現在の表示カーソルではなく state.plies の末尾）の結果をアーカイブ語彙で返す
function currentResult() {
  if (state.resultOverride) return state.resultOverride;
  const term = evaluateTerminalAt(state.plies.length);
  if (term.status === 'terminal') return { kind: term.kind, outcome: term.outcome };
  return { kind: 'unfinished', outcome: 'none' };
}

// 現在の対局を版タプル付きアーカイブ書式のテキストへ変換する。失敗時は null。
function buildArchiveText() {
  if (!state.versionTuple) return null;
  const request = {
    initial_sfen: INITIAL_SFEN,
    plies: state.plies.map(p => ({ s: p.sUsi, g: p.gUsi })),
    rule: state.versionTuple.rule,
    protocol: state.versionTuple.protocol,
    app: state.versionTuple.app,
    sente: null,
    gote: null,
    result: currentResult(),
  };
  const text = wasmBuildArchive(JSON.stringify(request));
  if (text.startsWith('ERROR:')) {
    console.error('build_archive failed:', text);
    return null;
  }
  return text;
}

function archiveFilename() {
  const now = new Date();
  const pad = n => String(n).padStart(2, '0');
  const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}` +
    `_${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `fukanzen-shogi_${stamp}.kifu`;
}

async function saveKifu() {
  const text = buildArchiveText();
  if (!text) { alert('棋譜の保存に失敗しました'); return; }

  const blob = new Blob([text], { type: 'text/plain' });
  const url  = URL.createObjectURL(blob);
  const a    = document.createElement('a');
  a.href     = url;
  a.download = archiveFilename();
  a.click();
  URL.revokeObjectURL(url);

  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // クリップボード API が使えない環境ではダウンロードのみで良しとする
  }
}

// アーカイブ書式（または旧 sfen 始まり棋譜）のテキストをパースする。
// 常に { ok, error?, initial_sfen?, plies?, meta? } を返す（例外を投げない）。
function parseArchiveText(text) {
  try {
    return JSON.parse(wasmParseArchive(text));
  } catch {
    // wasm 側の手組み JSON がもし壊れていても（本来は起きない想定）、
    // ここで確実に食い止めて穏当な失敗にする（多層防御）。
    return { ok: false, error: 'invalid_json' };
  }
}

// maxTurns は init() 完了後に engine-wasm から取得されるため、ここでは
// 呼び出し時点の値を参照する関数にする（モジュール読込時点の定数にしない）。
function archiveLoadErrorJa(error) {
  if (error === 'too_many_plies') {
    return `棋譜の着手数が多すぎます（上限 ${state.maxTurns} 組手）。読み込みを中止しました。`;
  }
  return '棋譜を読み込めませんでした';
}

// 読み込んだアーカイブを既存の再生機構（棋譜ナビ・水墨盤・日本語表記）へ流し込む。
function loadArchive(text) {
  const parsed = parseArchiveText(text);
  if (!parsed.ok) {
    alert(archiveLoadErrorJa(parsed.error));
    return;
  }

  const plies = parsed.plies.map(p => ({ sUsi: p.s, gUsi: p.g }));
  if (!pliesAreAllLegal(parsed.initial_sfen, plies)) {
    alert('棋譜を読み込めませんでした（非合法な着手を含む棋譜です）');
    return;
  }

  // ローカル鑑賞として読む（オンライン状態は畳む）
  if (state.onlineMode || state.onlineGameOver) { _resetOnlineState(); disconnectOnline(); }

  try {
    loadPlies(plies, parsed.initial_sfen);
  } catch (e) {
    // 上の pliesAreAllLegal で弾いているので通常は到達しない。多層防御として残す。
    alert('棋譜の再生に失敗しました: ' + e.message);
    return;
  }

  update({ loadedMeta: parsed.meta });
}

// ── Watch mode（淀川第三歩・観戦） ───────────────────────────────────────────────

function _metaToLoadedMeta(version, result) {
  if (!version) return null;
  return {
    rule: version.rule, protocol: version.protocol, app: version.app,
    sente: null, gote: null,
    result: result ?? { kind: 'unfinished', outcome: 'none' },
  };
}

// 観戦トークンで部屋へ読み取り専用接続し、第二歩の再生機構へ流し込む。
function enterWatchMode(token) {
  if (state.onlineMode) { _resetOnlineState(); disconnectOnline(); }
  resetToNew();
  update({ watchMode: true, watchStatusText: '', recordStatusText: '', archivedLink: null });

  connectSpectate(token, {
    onStatus: (statusText) => {
      update({ watchStatusText: statusText });
    },
    onInit: ({ version, initial_sfen, turns, result }) => {
      const plies = turns.map(t => ({ sUsi: t.s, gUsi: t.g }));
      try {
        loadPlies(plies, initial_sfen || INITIAL_SFEN);
      } catch (e) {
        console.error('watch: catchup replay failed:', e.message);
      }
      update({
        cursor: state.plies.length,  // 現局面（最新）まで追いつく
        loadedMeta: _metaToLoadedMeta(version, result),
      });
    },
    onMeta: ({ version, initial_sfen }) => {
      // 同じ部屋で新しい対局（再戦）が始まった。記録を初期化して迎える。
      resetToNew();
      update({
        sfens: [initial_sfen || INITIAL_SFEN],
        loadedMeta: _metaToLoadedMeta(version, null),
        recordStatusText: '',
        archivedLink: null,
      });
    },
    onTurn: (sUsi, gUsi) => {
      watchAppendTurn(sUsi, gUsi);
      render();
    },
    onResult: (kind, outcome) => {
      if (state.loadedMeta) state.loadedMeta.result = { kind, outcome };
      render();
    },
    onRecordConfirmed: () => {
      // 記録係二段目 §10: 記録係がこの対局に招かれたことを観戦者にも透明に示す。
      update({ recordStatusText: '記録係: 有効（この対局は書庫へ綴じられます）' });
    },
    onRecordDisagreement: (idA, idB, id) => {
      update({
        recordStatusText: '記録が食い違いました（裁定はされません）',
        archivedLink: id ? { id, url: archiveUrl(id) } : null,
      });
    },
    onArchived: (id) => {
      update({ recordStatusText: '記録されました', archivedLink: { id, url: archiveUrl(id) } });
    },
  });
}

function leaveWatchMode() {
  disconnectSpectate();
  resetToNew();
  update({ watchMode: false, recordStatusText: '', archivedLink: null, watchStatusText: '' });
}

// ── Input management ──────────────────────────────────────────────────────────

function resetInput() {
  state.inputStep        = null;
  state.pendingSente     = null;
  state.pendingGote      = null;
  state.selectedFrom     = null;
  state.legalTargets     = null;
  state.promotionPending = null;
  hidePromotionUI();
}

function getLegalMovesForSide(side) {
  const sfen = state.sfens[state.cursor];
  if (state.legalCache.sfen !== sfen) {
    state.legalCache = { sfen, sente: null, gote: null };
  }
  if (!state.legalCache[side]) {
    state.legalCache[side] = JSON.parse(wasmLegalActions(sfen, side)).map(parseUsi);
  }
  return state.legalCache[side];
}

function activateMoves(moves, from) {
  if (!moves.length) { state.selectedFrom = null; state.legalTargets = null; return; }
  state.selectedFrom = from;
  state.legalTargets = buildTargetMap(moves);
}

function selectBoardPiece(file, rank) {
  if (!state.inputStep) state.inputStep = 'sente';
  const side  = state.inputStep === 'gote' ? 'gote' : 'sente';
  const moves = movesFromSquare(getLegalMovesForSide(side), file, rank);
  activateMoves(moves, { board: [file, rank] });
}

function selectHandPiece(kind) {
  if (!state.inputStep) state.inputStep = 'sente';
  const side  = state.inputStep === 'gote' ? 'gote' : 'sente';
  const moves = dropsOfKind(getLegalMovesForSide(side), kind);
  activateMoves(moves, { hand: kind });
}

function selectTarget(file, rank) {
  const action = resolveTarget(state.legalTargets, file, rank);
  if (action.kind === 'deselect') {
    state.selectedFrom = null; state.legalTargets = null;
  } else if (action.kind === 'promptPromotion') {
    state.promotionPending = { options: action.options, toSquare: action.toSquare };
    showPromotionUI();
  } else { // 'confirm'
    confirmMove(action.usi);
  }
}

function confirmMove(usi) {
  const side = state.inputStep === 'gote' ? 'gote' : 'sente';
  const text = usiToText(usi, state.sfens[state.cursor], side);

  if (state.onlineMode) {
    // オンラインモード: 自分の陣営だけ確定して commit を送信する
    if (side === 'sente') state.pendingSente = { usi, text };
    else                  state.pendingGote  = { usi, text };
    hidePromotionUI();
    commitMoveOnline(state.sfens[state.cursor], usi);
    update({
      inputStep: null, selectedFrom: null, legalTargets: null,
      promotionPending: null, onlineCommitted: true,
    });
    return;
  }

  // ホットシートモード（従来）: 純粋遷移へ委譲
  hidePromotionUI();
  update(hotseatConfirmReduce(side, { usi, text }));
}

function _resetOnlineState() {
  // 呼び出し元（disconnectOnline/resetToNew と組で呼ばれる）が後で描画するため、
  // ここでは render しない（update ではなく Object.assign を直接使う）。
  Object.assign(state, resetOnlineReduce());
}

function endOnlineGame(msg) {
  // 観戦者へライブの終局表示を知らせる（disconnectOnline で ws を閉じる前に
  // 送る必要がある。text は同梱しない——綴じは record_testimony 経路へ移った。
  // 記録係二段目 §10）。currentResult() は onlineGameOver 等を参照しないため、
  // state 更新（末尾の update）より先に呼んでも結果は変わらない。
  const result = currentResult();
  sendSpectateResult(result.kind, result.outcome);
  if (isRecording()) {
    // 両陣営が証言として正準本文を送る（§3・§9）。二証人の突き合わせは DO 側の
    // 非同期処理（ハッシュ計算・KV 書き込み）を要するため、すぐ切断すると
    // archived/record_disagreement の通知（§5）を受け取れずに終わる——両者が
    // 証言を送った直後に自分から切断してしまうため。結果が届く（onArchived/
    // onRecordDisagreement）まで、または保険のタイムアウトまで待ってから切断する。
    sendRecordTestimony(result.kind, result.outcome, buildArchiveText());
    state._pendingRecordDisconnect = true;
    setTimeout(() => {
      if (state._pendingRecordDisconnect) { state._pendingRecordDisconnect = false; disconnectOnline(); }
    }, 5000);
  } else {
    // 終局後は WS を閉じる（intentional なので onlineMode は破棄しない）
    disconnectOnline();
  }
  update({ onlineGameOver: true, onlineEndMsg: msg, onlineCommitted: false, onlineWaiting: false });
}

function handleTurnComplete(senteUsi, goteUsi) {
  state.onlineCommitted = false;

  // 投了の検出（ルール 5.3 / 5.4）
  const sResign = senteUsi === 'resign';
  const gResign = goteUsi  === 'resign';
  if (sResign || gResign) {
    let msg, outcome;
    if (sResign && gResign) {
      msg = '引き分け（両者投了）';
      outcome = 'draw';
    } else if (sResign) {
      msg = state.onlineSide === 'sente' ? '投了しました（後手の勝ち）' : '相手が投了しました（先手の勝ち）';
      outcome = 'gote_wins';
    } else {
      msg = state.onlineSide === 'gote'  ? '投了しました（先手の勝ち）' : '相手が投了しました（後手の勝ち）';
      outcome = 'sente_wins';
    }
    state.resultOverride = { kind: 'resign', outcome };
    endOnlineGame(msg);
    return;
  }

  // resolve_ply（engine::resolve()）へ渡す前の安全弁。相手の reveal はここまで
  // 拘束性・盤面ハッシュしか検証されておらず合法性は未検証なので、ここで確認する
  // ——さもないと空マスからの移動や成れない駒の成り宣言のような非合法な reveal で
  // resolve_ply が wasm パニックを起こしうる（不正な相手からの攻撃面）。
  if (!turnActionsAreLegal(state.sfens[state.cursor], senteUsi, goteUsi)) {
    abortOnline('相手から非合法な着手を受信しました');
    endOnlineGame('中断: 相手から非合法な着手を受信しました');
    return;
  }

  const sText = usiToText(senteUsi, state.sfens[state.cursor], 'sente');
  const gText = usiToText(goteUsi,  state.sfens[state.cursor], 'gote');
  branchAndAppend(senteUsi, goteUsi, sText, gText);
  // phase='reveal' のまま待機 → 盤面クリックで次局面へ（handleSvgClick で処理）
  render();
}

// ── Promotion UI ──────────────────────────────────────────────────────────────

function showPromotionUI() {
  document.getElementById('promotion-overlay').classList.add('visible');
}

function hidePromotionUI() {
  document.getElementById('promotion-overlay')?.classList.remove('visible');
}

// ── Click handling ────────────────────────────────────────────────────────────

function svgCoords(event) {
  const svg  = document.getElementById('board');
  const rect = svg.getBoundingClientRect();
  return {
    x: (event.clientX - rect.left) * (SVG_W / rect.width),
    y: (event.clientY - rect.top)  * (SVG_H / rect.height),
  };
}

function getBoardSquare(sx, sy) {
  if (sx < BX || sx >= BX + BW || sy < BY || sy >= BY + BH) return null;
  const file = 9 - Math.floor((sx - BX) / CELL);
  const rank = Math.floor((sy - BY) / CELL) + 1;
  if (file < 1 || file > 9 || rank < 1 || rank > 9) return null;
  return [file, rank];
}

function getHandPieceAt(hand, y0, sx, sy) {
  if (sy < y0 - 4 || sy > y0 + PFS + 4) return null;
  let ox = BX + 74;
  for (const k of HAND_ORDER) {
    const cnt = hand[k] || 0;
    if (cnt <= 0) continue;
    const w = (KANJI[k] + countStr(cnt)).length * PFS;
    if (sx >= ox - 2 && sx <= ox + w + 2) return k;
    ox += w + 4;
  }
  return null;
}

function _advanceFromReveal(sx, sy) {
  state.cursor++;
  state.phase = 'position';
  const msg = getGameOverMsg();
  if (msg) { endOnlineGame(msg); return; }

  if (state.onlineSide === 'gote') state.inputStep = 'gote';

  // クリック座標が自分の合法手の駒に当たっていれば選択状態へ直接遷移
  const activeSide = state.onlineSide === 'gote' ? 'g' : 's';
  const pos = parseSfen(state.sfens[state.cursor]);
  const sq  = getBoardSquare(sx, sy);
  if (sq) {
    const [f, r] = sq;
    const piece = pos.board.get(`${f},${r}`);
    if (piece && piece.side === activeSide) selectBoardPiece(f, r);
  } else if (state.onlineSide === 'gote') {
    const k = getHandPieceAt(pos.handG, 8, sx, sy);
    if (k) selectHandPiece(k);
  } else {
    const k = getHandPieceAt(pos.handS, BY + BH + 12, sx, sy);
    if (k) selectHandPiece(k);
  }
  render();
}

function handleSvgClick(event) {
  if (state.watchMode) return;  // 観戦は読み取り専用（盤クリックで着手できない）

  // 同時開示フェーズ: 盤面・駒台クリックで次局面へ遷移
  if (state.phase === 'reveal' && state.onlineMode && !state.onlineGameOver) {
    const { x: sx, y: sy } = svgCoords(event);
    _advanceFromReveal(sx, sy);
    return;
  }

  if (state.phase !== 'position') return;
  if (state.promotionPending)     return;
  if (state.onlineMode && state.onlineCommitted) return;

  const { x: sx, y: sy } = svgCoords(event);
  const gameOver = getGameOverMsg();
  const pos      = parseSfen(state.sfens[state.cursor]);
  const activeSide = state.inputStep === 'gote' ? 'g' : 's';

  // If target selection is active, check for legal target click first
  if (state.legalTargets) {
    const sq = getBoardSquare(sx, sy);
    if (sq) {
      const key = `${sq[0]},${sq[1]}`;
      if (state.legalTargets.has(key)) { selectTarget(sq[0], sq[1]); render(); return; }
    }
  }

  // Clicks disabled when game is over and no input is active
  if (gameOver && !state.inputStep) return;

  // Board square click
  const sq = getBoardSquare(sx, sy);
  if (sq) {
    const [f, r] = sq;
    const piece  = pos.board.get(`${f},${r}`);
    if (piece && piece.side === activeSide) {
      // Toggle selection on same piece; switch to different own piece
      if (state.selectedFrom?.board?.[0] === f && state.selectedFrom?.board?.[1] === r) {
        update({ selectedFrom: null, legalTargets: null });
      } else {
        selectBoardPiece(f, r);
        render();
      }
      return;
    }
    // Clicked empty or opponent square → deselect without changing inputStep
    if (state.selectedFrom) update({ selectedFrom: null, legalTargets: null });
    return;
  }

  // Gote hand (y=8) — only during gote's turn
  if (state.inputStep === 'gote') {
    const k = getHandPieceAt(pos.handG, 8, sx, sy);
    if (k) {
      if (state.selectedFrom?.hand === k) {
        update({ selectedFrom: null, legalTargets: null });
      } else {
        selectHandPiece(k);
        render();
      }
      return;
    }
  }

  // Sente hand (y=BY+BH+12) — during sente's turn or before input starts
  if (state.inputStep !== 'gote') {
    const k = getHandPieceAt(pos.handS, BY + BH + 12, sx, sy);
    if (k) {
      if (state.selectedFrom?.hand === k) {
        update({ selectedFrom: null, legalTargets: null });
      } else {
        selectHandPiece(k);
        render();
      }
      return;
    }
  }
}

// ── Overlay computation ────────────────────────────────────────────────────────

// ── Navigation ────────────────────────────────────────────────────────────────

// navReduce（純粋）が受ける、state から必要部分だけを切り出したスナップショット。
function navView() {
  return {
    phase: state.phase, cursor: state.cursor, pliesLen: state.plies.length,
    onlineMode: state.onlineMode, onlineGameOver: state.onlineGameOver,
  };
}

function goNext() {
  if (state.promotionPending) return;
  if (state.onlineMode && !state.onlineGameOver) return; // 対局中はナビ不可

  if (state.pendingSente && state.pendingGote) {
    branchAndAppend(state.pendingSente.usi, state.pendingGote.usi, state.pendingSente.text, state.pendingGote.text);
    render(); return;
  }

  const patch = navReduce(navView(), 'next');
  if (patch) update(patch); else render();
}

function goPrev() {
  // 副作用分岐（純粋化しない）: 入力途中のキャンセルを優先
  if (state.onlineMode && !state.onlineGameOver) return; // ナビ不可（navReduce と同判定だが早期 return）
  if (state.promotionPending) {
    hidePromotionUI();
    update({ promotionPending: null, selectedFrom: null, legalTargets: null });
    return;
  }

  if (state.inputStep !== null || state.selectedFrom !== null) {
    // One press cancels all input state; second press starts navigating back
    resetInput();
    render();
    return;
  }

  const patch = navReduce(navView(), 'prev');
  if (patch) update(patch); else render();
}

// ── Render ────────────────────────────────────────────────────────────────────

function render() {
  const pos       = parseSfen(state.sfens[state.cursor]);
  const bothReady = !!(state.pendingSente && state.pendingGote);
  const hasInput  = !!(state.inputStep || state.selectedFrom || state.pendingSente || state.pendingGote);
  const gameOver  = getGameOverMsg();

  const overlay = state.phase === 'reveal'
    ? revealOverlay(state.plies[state.cursor])
    : (hasInput
        ? inputOverlay({ selectedFrom: state.selectedFrom, inputStep: state.inputStep, legalTargets: state.legalTargets })
        : null);

  const { phaseText, moveText, eventText, archiveInfo, step, total } = labelView(state, gameOver);

  const svg = document.getElementById('board');
  svg.setAttribute('viewBox', `0 0 ${SVG_W} ${SVG_H}`);
  svg.innerHTML = renderSvg(pos, overlay);
  svg.style.cursor = (state.phase === 'position' && !gameOver && !state.watchMode && !(state.onlineMode && state.onlineCommitted))
    ? 'pointer' : 'default';

  document.getElementById('phase-label').textContent  = phaseText;
  document.getElementById('move-display').textContent = moveText;
  document.getElementById('event-label').textContent  = eventText || ' ';

  const archiveInfoEl = document.getElementById('archive-info');
  archiveInfoEl.textContent = archiveInfo.text;
  archiveInfoEl.classList.toggle('mismatch', archiveInfo.mismatch);

  document.getElementById('step-label').textContent = `${step} / ${total}`;

  const btnNext = document.getElementById('btn-next');
  const btnPrev = document.getElementById('btn-prev');

  if (state.watchMode) {
    // 観戦中は常に棋譜ナビゲーション可能（コミット待ちの概念が無い）。
    btnNext.textContent = '次 →';
    btnNext.disabled = !(
      state.phase === 'reveal' ||
      (state.phase === 'position' && state.cursor < state.plies.length)
    );
    btnPrev.disabled = state.cursor === 0 && state.phase === 'position';
  } else if (state.onlineMode) {
    btnNext.textContent = '次 →';
    if (state.onlineGameOver) {
      // 終局後は棋譜ナビゲーションを解放（phase に関係なく維持）
      btnNext.disabled = !(
        state.phase === 'reveal' ||
        (state.phase === 'position' && state.cursor < state.plies.length)
      );
      btnPrev.disabled = state.cursor === 0 && state.phase === 'position';
    } else {
      btnNext.disabled = true;
      btnPrev.disabled = true;
    }
  } else {
    btnNext.textContent = bothReady ? '解決 →' : '次 →';
    btnNext.disabled    = !(
      bothReady ||
      state.phase === 'reveal' ||
      (state.phase === 'position' && !hasInput && state.cursor < state.plies.length)
    );
    btnPrev.disabled    = (
      state.cursor === 0 && state.phase === 'position' && !hasInput && !state.promotionPending
    );
  }

  const btnResign = document.getElementById('btn-resign');
  if (btnResign) {
    btnResign.style.display = (state.onlineMode && !state.onlineGameOver) ? 'inline-block' : 'none';
    btnResign.disabled      = state.onlineCommitted || state.onlineWaiting;
  }

  const btnSave = document.getElementById('btn-save');
  if (btnSave) {
    const isOver = state.onlineMode ? state.onlineGameOver : !!gameOver;
    btnSave.classList.toggle('highlight', isOver);
  }

  // 観戦中は対局を始める系のボタンを封じ、代わりに「観戦をやめる」を出す。
  for (const id of ['btn-online', 'btn-load']) {
    const btn = document.getElementById(id);
    if (btn) btn.disabled = state.watchMode;
  }
  const btnLeaveWatch = document.getElementById('btn-leave-watch');
  if (btnLeaveWatch) btnLeaveWatch.hidden = !state.watchMode;

  const linkText = document.getElementById('watch-link-text');
  const linkBtn  = document.getElementById('btn-copy-watch-link');
  if (linkText && linkBtn) {
    if (state.onlineMode && state.spectateToken) {
      const link = `${location.origin}${location.pathname}?watch=${encodeURIComponent(state.spectateToken)}`;
      linkText.textContent = `観戦リンク: ${link}`;
      linkBtn.hidden = false;
      linkBtn.dataset.link = link;
    } else {
      linkText.textContent = '';
      linkBtn.hidden = true;
    }
  }

  // 記録係の状態表示（最小 surface。記録係二段目 §5）。
  const recordText = document.getElementById('record-info-text');
  const recordBtn   = document.getElementById('btn-copy-record-link');
  if (recordText && recordBtn) {
    recordText.textContent = state.recordStatusText;
    if (state.archivedLink) {
      recordBtn.hidden = false;
      recordBtn.dataset.link = state.archivedLink.url;
    } else {
      recordBtn.hidden = true;
    }
  }
}

// ── Init ──────────────────────────────────────────────────────────────────────

// Escape キーと閉じるボタンの両方から呼べるようモジュールスコープに置く
let closeModal     = () => {};
let closeLoadModal  = () => {};

document.addEventListener('DOMContentLoaded', async () => {
  window.__fukanzenDebug = () => console.table(debugState());
  document.getElementById('board').addEventListener('click', handleSvgClick);
  document.getElementById('btn-next').addEventListener('click', goNext);
  document.getElementById('btn-prev').addEventListener('click', goPrev);
  document.getElementById('btn-save').addEventListener('click', saveKifu);
  document.getElementById('btn-leave-watch').addEventListener('click', () => {
    leaveWatchMode();
  });
  document.getElementById('btn-copy-watch-link').addEventListener('click', async (e) => {
    const link = e.currentTarget.dataset.link;
    if (!link) return;
    try {
      await navigator.clipboard.writeText(link);
    } catch {
      // クリップボード API が使えない環境では選択して手動コピーしてもらう
    }
  });
  document.getElementById('btn-copy-record-link').addEventListener('click', async (e) => {
    const link = e.currentTarget.dataset.link;
    if (!link) return;
    try {
      await navigator.clipboard.writeText(link);
    } catch {
      // クリップボード API が使えない環境では選択して手動コピーしてもらう
    }
  });

  document.getElementById('btn-promote').addEventListener('click', () => {
    if (!state.promotionPending) return;
    const usi = state.promotionPending.options.find(o => o.promote)?.usi;
    if (usi) confirmMove(usi);
  });
  document.getElementById('btn-no-promote').addEventListener('click', () => {
    if (!state.promotionPending) return;
    const usi = state.promotionPending.options.find(o => !o.promote)?.usi;
    if (usi) confirmMove(usi);
  });

  document.addEventListener('keydown', e => {
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') goNext();
    if (e.key === 'ArrowLeft'  || e.key === 'ArrowUp')   goPrev();
    if (e.key === 'Escape') {
      const onlineModalEl = document.getElementById('online-modal');
      const loadModalEl   = document.getElementById('load-modal');
      if (onlineModalEl.classList.contains('visible')) {
        closeModal();
      } else if (loadModalEl.classList.contains('visible')) {
        closeLoadModal();
      } else {
        resetInput(); render();
      }
    }
  });

  // ── オンライン対戦モーダル ──────────────────────────────────────────────────
  {
    const modal     = document.getElementById('online-modal');
    const statusEl  = document.getElementById('online-status');
    const btnConn   = document.getElementById('btn-connect');
    const btnClose  = document.getElementById('btn-online-close');

    closeModal = () => {
      if (!state.onlineGameOver) {
        disconnectOnline();
        if (state.onlineMode) { _resetOnlineState(); resetToNew(); }
      }
      modal.classList.remove('visible');
      statusEl.textContent = '—';
      btnConn.disabled = false;
      btnConn.textContent = '入室';
      render();
    };

    document.getElementById('btn-resign').addEventListener('click', () => {
      if (!state.onlineMode || state.onlineGameOver || state.onlineCommitted) return;
      if (!confirm('投了しますか？')) return;
      // 投了は commit-reveal プロトコル経由。即終局にしない（両者投了の引き分けを拾うため）
      commitMoveOnline(state.sfens[state.cursor], 'resign');
      update({ onlineCommitted: true });
    });

    document.getElementById('btn-online').addEventListener('click', () => {
      // 前回の対局が終局済みなら畳んでから新しい接続へ（「新局」ボタンが担っていた役割）
      if (state.onlineGameOver) { _resetOnlineState(); resetToNew(); }
      modal.classList.add('visible');
      document.getElementById('input-room').focus();
    });

    btnClose.addEventListener('click', closeModal);

    btnConn.addEventListener('click', async () => {
      // 「再接続」ボタンとして機能する場合（self_disconnected 後）
      if (btnConn.textContent === '再接続' && hasReconnectableSession()) {
        btnConn.disabled = true;
        statusEl.textContent = '再接続中…';
        await reconnectOnline();
        return;
      }

      const roomKey = document.getElementById('input-room').value.trim();
      const secret  = document.getElementById('input-secret').value;
      if (!roomKey) {
        statusEl.textContent = 'ルームキーを入力してください';
        return;
      }
      btnConn.disabled = true;
      statusEl.textContent = '接続中…';

      const callbacks = {
        onStatus: (connStatus, msg) => {
          statusEl.textContent = msg;

          if (connStatus === 'ready') {
            if (!state.onlineMode) {
              // 初回接続: オンラインモード開始
              state.onlineMode       = true;
              state.onlineSide       = getMySide();
              state.onlineCommitted  = false;
              state.onlineGameOver   = false;
              state.onlineEndMsg     = '';
              state.onlineWaiting    = false;
              state.onlineWaitingMsg = '';
              state.spectateToken    = null;
              resetToNew();
              if (state.onlineSide === 'gote') state.inputStep = 'gote';
              sendSpectateMeta(state.versionTuple, state.sfens[0]);
            } else {
              // 再接続完了: ゲーム状態はそのまま、waiting 解除
              state.onlineWaiting    = false;
              state.onlineWaitingMsg = '';
              state.onlineCommitted  = false;
            }
            modal.classList.remove('visible');
            btnConn.disabled = false;
            btnConn.textContent = '入室';
            render();

            // 記録係への招待の prompt（対局開始・握手完了時。記録係二段目 §5）。
            // モーダルが閉じて盤面が見えた後に出す。招き忘れ対策——オプトイン
            // だが必ず尋ねる。相手も同時に自分の招待を出しうる（どちらから
            // 提案してもよい。§2）ので、二重の招待が交差しても害はない。
            if (!state.recordInviteAsked) {
              state.recordInviteAsked = true;
              setTimeout(() => {
                if (confirm('記録係をこの対局に招いて綴じてもらいますか？（相手の同意が必要です）')) {
                  sendRecordInvite();
                }
              }, 0);
            }

          } else if (connStatus === 'peer_disconnected') {
            // 相手が切断: ゲーム状態維持、待機表示
            update({ onlineWaiting: true, onlineWaitingMsg: msg, onlineCommitted: false });

          } else if (connStatus === 'self_disconnected') {
            // 自分が切断: 再接続可能な状態で待機
            btnConn.disabled = false;
            btnConn.textContent = '再接続';
            update({ onlineWaiting: true, onlineWaitingMsg: msg, onlineCommitted: false });

          } else if (connStatus === 'error') {
            btnConn.disabled = false;
            btnConn.textContent = '入室';
            if (state.onlineMode && !state.onlineGameOver) {
              update({ onlineWaiting: true, onlineWaitingMsg: `エラー: ${msg}` });
            } else {
              render();
            }

          } else if (connStatus === 'disconnected') {
            if (!state.onlineGameOver) _resetOnlineState();
            btnConn.disabled = false;
            btnConn.textContent = '入室';
            render();
          }
        },
        onTurnComplete:  handleTurnComplete,
        onPeerAborted:   (reason) => endOnlineGame(`中断: ${reason}`),
        onSpectateToken: (token) => { update({ spectateToken: token }); },
        onRecordInvite: () => {
          // 相手からの記録係への招待提案（記録係二段目 §2・§5）。
          if (confirm('相手が記録係をこの対局に招いて綴じることを提案しました。同意しますか？')) {
            sendRecordAccept();
          } else {
            sendRecordDecline();
          }
        },
        onRecordConfirmed: () => {
          update({ recordStatusText: '記録係: 有効（この対局は書庫へ綴じられます）' });
        },
        onRecordDeclined: () => {
          alert('相手が記録を辞退しました。この対局は綴じられません。');
          update({ recordStatusText: '' });
        },
        onRecordDisagreement: (idA, idB, id) => {
          state.recordStatusText = '記録が食い違いました（裁定はされません）';
          state.archivedLink = id ? { id, url: archiveUrl(id) } : null;
          alert('二人の証言が一致しませんでした。改竄検知として記録し、裁定はしません。');
          if (state._pendingRecordDisconnect) { state._pendingRecordDisconnect = false; disconnectOnline(); }
          render();
        },
        onArchived: (id) => {
          if (state._pendingRecordDisconnect) { state._pendingRecordDisconnect = false; disconnectOnline(); }
          update({ recordStatusText: '記録されました', archivedLink: { id, url: archiveUrl(id) } });
        },
        getSfens:        () => state.sfens,
        onResumeAt:      (resumeSfen) => {
          const idx = state.sfens.indexOf(resumeSfen);
          if (idx >= 0) {
            resetInput();  // selectedFrom・legalTargets 等をクリア（inputStep は null になる）
            update({
              cursor: idx,
              phase: 'position',
              onlineWaiting: false,
              onlineWaitingMsg: '',
              onlineCommitted: false,
              inputStep: state.onlineSide === 'gote' ? 'gote' : 'sente',
            });
          } else {
            render();
          }
        },
      };

      await connectOnline(roomKey, secret, callbacks);
    });
  }

  // ── 棋譜読込モーダル ────────────────────────────────────────────────────────
  {
    const modal      = document.getElementById('load-modal');
    const inputFile  = document.getElementById('input-file');
    const inputPaste = document.getElementById('input-paste');

    closeLoadModal = () => {
      modal.classList.remove('visible');
      inputPaste.value = '';
      inputFile.value  = '';
    };

    document.getElementById('btn-load').addEventListener('click', () => {
      modal.classList.add('visible');
    });
    document.getElementById('btn-load-close').addEventListener('click', closeLoadModal);

    document.getElementById('btn-load-file').addEventListener('click', () => inputFile.click());
    inputFile.addEventListener('change', () => {
      const file = inputFile.files?.[0];
      if (!file) return;
      if (file.size > MAX_ARCHIVE_BYTES) {
        alert(`ファイルが大きすぎます（上限 ${MAX_ARCHIVE_BYTES / 1024} KB）。読み込みを中止しました。`);
        inputFile.value = '';
        return;
      }
      const reader = new FileReader();
      reader.onload = () => {
        loadArchive(String(reader.result));
        closeLoadModal();
      };
      reader.readAsText(file);
    });

    document.getElementById('btn-load-paste').addEventListener('click', () => {
      const text = inputPaste.value.trim();
      if (!text) return;
      if (text.length > MAX_ARCHIVE_BYTES) {
        alert(`テキストが大きすぎます（上限 ${MAX_ARCHIVE_BYTES / 1024} KB）。読み込みを中止しました。`);
        return;
      }
      loadArchive(text);
      closeLoadModal();
    });
  }

  document.getElementById('phase-label').textContent = '読み込み中…';
  document.getElementById('btn-prev').disabled = true;
  document.getElementById('btn-next').disabled = true;

  try {
    await Promise.all([init(), initNotation(), initProtocol()]);
    state.versionTuple = JSON.parse(wasmVersionTuple());
    state.maxTurns = wasmMaxTurns();

    const watchToken = new URLSearchParams(location.search).get('watch');
    if (watchToken) {
      enterWatchMode(watchToken);
    } else {
      resetToNew();
      render();
    }
  } catch (err) {
    document.getElementById('phase-label').textContent = `読み込みエラー: ${err.message}`;
    console.error(err);
  }
});
