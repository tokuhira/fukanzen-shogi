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
  connectOnline, disconnectOnline, commitMoveOnline, getMySide,
  reconnectOnline, hasReconnectableSession, debugState,
  sendSpectateMeta, sendSpectateResult, connectSpectate, disconnectSpectate,
  sendRecordInvite, sendRecordAccept, sendRecordDecline, sendRecordTestimony,
  isRecording, archiveUrl,
} from './online.js';

import { parseUsi } from './usi.js';
import { formatResult, terminalMessageJa } from './result-view.js';
import {
  CELL, BX, BY, BW, BH, SVG_W, SVG_H, PFS, KANJI, HAND_ORDER, countStr,
} from './geometry.js';
import { renderSvg } from './board-view.js';

// ── Constants ─────────────────────────────────────────────────────────────────

const INITIAL_SFEN = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

// 読込を受け付けるアーカイブテキストの最大バイト数（安全弁）。
// 500組手の正当なアーカイブは概算 13〜14KB（着手行 500×24B＋ヘッダ約1.2KB）。
// 巨大な悪意あるファイルでブラウザを固まらせないための、十分な安全マージン。
const MAX_ARCHIVE_BYTES = 512 * 1024;

const EVENT_LABEL = {
  clash:      '相討ち',
  sente_died: '先手玉が取られた',
  gote_died:  '後手玉が取られた',
  both_died:  '両玉相討ち',
};

function usiToText(usi, sfen, side) {
  const prefix = side === 'sente' ? '☗' : '☖';
  const legalJson = wasmLegalActions(sfen, side);
  return `${prefix}${wasmJaNotation(usi, side, legalJson, sfen)}`;
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

// plies[i] = { sUsi, gUsi, sText, gText }
const kifu = { plies: [] };
let cursor = 0;
let sfens  = [INITIAL_SFEN];  // sfens[i] = position entering turn i
let events = [];               // events[i] = event string from resolving plies[i]
let phase  = 'position';       // 'position' | 'reveal'

// ── Input state ───────────────────────────────────────────────────────────────

let inputStep        = null;  // null | 'sente' | 'gote'
let pendingSente     = null;  // null | { usi, text }
let pendingGote      = null;  // null | { usi, text }
let selectedFrom     = null;  // null | { board:[f,r] } | { hand:kind }
let legalTargets     = null;  // null | Map<"f,r", { options:[{usi,promote}] }>
let promotionPending = null;  // null | { options, toSquare }

// Per-sfen legal move cache
let legalCache = { sfen: null, sente: null, gote: null };

// Per-cursor game-over cache
let gameOverCache = { cursor: -1, msg: null };

// ── Archive state ─────────────────────────────────────────────────────────────

let versionTuple    = null;  // { rule, protocol, app } — init() 完了後にキャッシュ
let resultOverride  = null;  // { kind, outcome } | null — 投了など盤面から導出できない結果
let loadedMeta       = null;  // 読み込んだアーカイブの ArchiveMeta（鑑賞表示・版不一致判定用）
let maxTurns         = null;  // ルール v0.6 の最長手数（組手）。init() 完了後に engine-wasm から取得

// ── Online mode state ─────────────────────────────────────────────────────────

let onlineMode            = false;
let onlineSide            = null;    // 'sente' | 'gote'
let onlineCommitted       = false;   // 自分の commit 送信済み（解決待ち中）
let onlineGameOver        = false;   // 終局確定（review 中も true を維持）
let onlineEndMsg          = '';      // 終局理由の表示文字列（投了時など）
let onlineWaiting         = false;   // 切断待機中（相手切断 or 自分切断後の再接続待ち）
let onlineWaitingMsg      = '';      // 待機時の表示メッセージ

// 対局時に受け取った観戦リンク用トークン（プレイヤー側。共有リンク表示に使う）
let spectateToken = null;

// ── 記録係の招待と二証人（記録係二段目） ─────────────────────────────────────

let recordInviteAsked = false;  // このゲームで招待の可否を既に尋ねたか（二重prompt防止）
let recordStatusText  = '';     // 記録係の状態表示用テキスト（最小 surface。§5）
let archivedLink       = null;  // { id, url } 直近の archived 通知（GET /archive/:id へのリンク）
let _pendingRecordDisconnect = false;  // 証言送信後、綴じ結果を受け取るまで切断を待っているか

// ── Watch mode state（淀川第三歩） ───────────────────────────────────────────────

let watchMode       = false;  // 読み取り専用の観戦者として接続中か
let watchStatusText = '';     // 観戦接続の状態表示（接続中／切断など）

// ── Kifu management ───────────────────────────────────────────────────────────

function loadPlies(plies, initialSfen = INITIAL_SFEN) {
  kifu.plies = [];
  sfens  = [initialSfen];
  events = [];
  for (const ply of plies) {
    const preSfen = sfens.at(-1);
    const r = JSON.parse(resolve_ply(preSfen, ply.sUsi, ply.gUsi));
    if (!r.ok) throw new Error(r.error);
    // 日本語表記が無ければ再計算（中身は USI、表記は導出）
    const sText = ply.sText ?? usiToText(ply.sUsi, preSfen, 'sente');
    const gText = ply.gText ?? usiToText(ply.gUsi, preSfen, 'gote');
    kifu.plies.push({ sUsi: ply.sUsi, gUsi: ply.gUsi, sText, gText });
    sfens.push(r.sfen);
    events.push(r.event);
  }
  cursor = 0;
  phase  = 'position';
  resetInput();
  gameOverCache  = { cursor: -1, msg: null };
  resultOverride = null;
}

function resetToNew() {
  kifu.plies = [];
  sfens  = [INITIAL_SFEN];
  events = [];
  cursor = 0;
  phase  = 'position';
  resetInput();
  gameOverCache  = { cursor: -1, msg: null };
  resultOverride = null;
  loadedMeta     = null;
}

function branchAndAppend(sUsi, gUsi, sText, gText) {
  kifu.plies = kifu.plies.slice(0, cursor);
  sfens  = sfens.slice(0, cursor + 1);
  events = events.slice(0, cursor);
  gameOverCache = { cursor: -1, msg: null };

  const r = JSON.parse(resolve_ply(sfens[cursor], sUsi, gUsi));
  if (!r.ok) throw new Error(r.error);

  kifu.plies.push({ sUsi, gUsi, sText, gText });
  sfens.push(r.sfen);
  events.push(r.event);

  phase = 'reveal';  // cursor stays — reveal shows the move just played
  resetInput();
}

// 観戦: 受信した組手を kifu の末尾へ追記する（分岐しない・cursor は動かさない）。
// 利用者が過去局面をレビュー中でも、ライブの新着はそのまま棋譜の末尾に積まれる。
function watchAppendTurn(sUsi, gUsi) {
  const preSfen = sfens[kifu.plies.length];
  const r = JSON.parse(resolve_ply(preSfen, sUsi, gUsi));
  if (!r.ok) { console.error('watch: resolve_ply failed:', r.error); return; }
  const sText = usiToText(sUsi, preSfen, 'sente');
  const gText = usiToText(gUsi, preSfen, 'gote');
  kifu.plies.push({ sUsi, gUsi, sText, gText });
  sfens.push(r.sfen);
  events.push(r.event);
  gameOverCache = { cursor: -1, msg: null };
}

// ── Game-over detection ───────────────────────────────────────────────────────

function getGameOverMsg() {
  if (phase !== 'position') return null;
  if (cursor !== gameOverCache.cursor) {
    gameOverCache = { cursor, msg: computeGameOver() };
  }
  return gameOverCache.msg;
}

// kifu.plies の先頭から uptoPlies 組手までの局面を、engine::terminate::evaluate
// （ルール v0.6 §5.8 の一元評価）で判定する。盤上で導ける終局はすべてこの一箇所
// に集約し、web 側では順序を再実装しない（アーカイブ語彙 kind/outcome を返す）。
function evaluateTerminalAt(uptoPlies) {
  const request = {
    initial_sfen: sfens[0],
    plies: kifu.plies.slice(0, uptoPlies).map(p => ({ s: p.sUsi, g: p.gUsi })),
  };
  return JSON.parse(wasmEvaluateTerminal(JSON.stringify(request)));
}

function computeGameOver() {
  const term = evaluateTerminalAt(cursor);
  if (term.status !== 'terminal') return null;
  return terminalMessageJa(term.kind, term.outcome, maxTurns);
}

// ── Archive ────────────────────────────────────────────────────────────────────

// 対局全体（現在の表示カーソルではなく kifu.plies の末尾）の結果をアーカイブ語彙で返す
function currentResult() {
  if (resultOverride) return resultOverride;
  const term = evaluateTerminalAt(kifu.plies.length);
  if (term.status === 'terminal') return { kind: term.kind, outcome: term.outcome };
  return { kind: 'unfinished', outcome: 'none' };
}

// 現在の対局を版タプル付きアーカイブ書式のテキストへ変換する。失敗時は null。
function buildArchiveText() {
  if (!versionTuple) return null;
  const request = {
    initial_sfen: INITIAL_SFEN,
    plies: kifu.plies.map(p => ({ s: p.sUsi, g: p.gUsi })),
    rule: versionTuple.rule,
    protocol: versionTuple.protocol,
    app: versionTuple.app,
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
    return `棋譜の着手数が多すぎます（上限 ${maxTurns} 組手）。読み込みを中止しました。`;
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

  // ローカル鑑賞として読む（オンライン状態は畳む）
  if (onlineMode || onlineGameOver) { _resetOnlineState(); disconnectOnline(); }

  const plies = parsed.plies.map(p => ({ sUsi: p.s, gUsi: p.g }));
  try {
    loadPlies(plies, parsed.initial_sfen);
  } catch (e) {
    alert('棋譜の再生に失敗しました: ' + e.message);
    return;
  }

  loadedMeta = parsed.meta;
  render();
}

// 読み込んだアーカイブの版タプル・結果を鑑賞表示する。版不一致なら注意を返す。
function archiveInfoText() {
  if (!loadedMeta) return { text: '', mismatch: false };

  const versionLine = loadedMeta.app
    ? `ルール ${loadedMeta.rule} / プロトコル ${loadedMeta.protocol} / v${loadedMeta.app}`
    : `ルール ${loadedMeta.rule} / プロトコル ${loadedMeta.protocol}`;
  const resultLine = formatResult(loadedMeta.result);

  const mismatch = !!(versionTuple && loadedMeta.rule !== versionTuple.rule);
  if (!mismatch) {
    return { text: `${versionLine} — ${resultLine}`, mismatch: false };
  }
  const warning =
    `この棋譜はルール ${loadedMeta.rule} で指されました。現在の再生エンジンはルール ${versionTuple.rule} です。` +
    `再生結果が当時と異なる可能性があります。`;
  return { text: `${versionLine} — ${resultLine} ／ ${warning}`, mismatch: true };
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
  if (onlineMode) { _resetOnlineState(); disconnectOnline(); }
  watchMode       = true;
  watchStatusText = '';
  recordStatusText = '';
  archivedLink      = null;
  resetToNew();
  render();

  connectSpectate(token, {
    onStatus: (state) => {
      watchStatusText = state;
      render();
    },
    onInit: ({ version, initial_sfen, turns, result }) => {
      const plies = turns.map(t => ({ sUsi: t.s, gUsi: t.g }));
      try {
        loadPlies(plies, initial_sfen || INITIAL_SFEN);
      } catch (e) {
        console.error('watch: catchup replay failed:', e.message);
      }
      cursor     = kifu.plies.length;  // 現局面（最新）まで追いつく
      loadedMeta = _metaToLoadedMeta(version, result);
      render();
    },
    onMeta: ({ version, initial_sfen }) => {
      // 同じ部屋で新しい対局（再戦）が始まった。記録を初期化して迎える。
      resetToNew();
      sfens      = [initial_sfen || INITIAL_SFEN];
      loadedMeta = _metaToLoadedMeta(version, null);
      recordStatusText = '';
      archivedLink      = null;
      render();
    },
    onTurn: (sUsi, gUsi) => {
      watchAppendTurn(sUsi, gUsi);
      render();
    },
    onResult: (kind, outcome) => {
      if (loadedMeta) loadedMeta.result = { kind, outcome };
      render();
    },
    onRecordConfirmed: () => {
      // 記録係二段目 §10: 記録係がこの対局に招かれたことを観戦者にも透明に示す。
      recordStatusText = '記録係: 有効（この対局は書庫へ綴じられます）';
      render();
    },
    onRecordDisagreement: (idA, idB, id) => {
      recordStatusText = '記録が食い違いました（裁定はされません）';
      archivedLink = id ? { id, url: archiveUrl(id) } : null;
      render();
    },
    onArchived: (id) => {
      recordStatusText = '記録されました';
      archivedLink = { id, url: archiveUrl(id) };
      render();
    },
  });
}

function leaveWatchMode() {
  disconnectSpectate();
  watchMode       = false;
  recordStatusText = '';
  archivedLink      = null;
  watchStatusText = '';
  resetToNew();
}

// ── Input management ──────────────────────────────────────────────────────────

function resetInput() {
  inputStep        = null;
  pendingSente     = null;
  pendingGote      = null;
  selectedFrom     = null;
  legalTargets     = null;
  promotionPending = null;
  hidePromotionUI();
}

function getLegalMovesForSide(side) {
  const sfen = sfens[cursor];
  if (legalCache.sfen !== sfen) {
    legalCache = { sfen, sente: null, gote: null };
  }
  if (!legalCache[side]) {
    legalCache[side] = JSON.parse(wasmLegalActions(sfen, side)).map(parseUsi);
  }
  return legalCache[side];
}

function buildTargetMap(moves) {
  const map = new Map();
  for (const m of moves) {
    const key = `${m.to[0]},${m.to[1]}`;
    if (!map.has(key)) map.set(key, { options: [] });
    map.get(key).options.push({ usi: m.usi, promote: m.promote });
  }
  return map;
}

function activateMoves(moves, from) {
  if (!moves.length) { selectedFrom = null; legalTargets = null; return; }
  selectedFrom = from;
  legalTargets = buildTargetMap(moves);
}

function selectBoardPiece(file, rank) {
  if (!inputStep) inputStep = 'sente';
  const side  = inputStep === 'gote' ? 'gote' : 'sente';
  const moves = getLegalMovesForSide(side).filter(
    m => !m.isDrop && m.from[0] === file && m.from[1] === rank
  );
  activateMoves(moves, { board: [file, rank] });
}

function selectHandPiece(kind) {
  if (!inputStep) inputStep = 'sente';
  const side  = inputStep === 'gote' ? 'gote' : 'sente';
  const moves = getLegalMovesForSide(side).filter(
    m => m.isDrop && m.kind === kind.toUpperCase()
  );
  activateMoves(moves, { hand: kind });
}

function selectTarget(file, rank) {
  const key = `${file},${rank}`;
  if (!legalTargets?.has(key)) {
    selectedFrom = null; legalTargets = null; return;
  }
  const { options } = legalTargets.get(key);
  const hasPromote   = options.some(o =>  o.promote);
  const hasNoPromote = options.some(o => !o.promote);
  if (hasPromote && hasNoPromote) {
    promotionPending = { options, toSquare: [file, rank] };
    showPromotionUI();
  } else {
    confirmMove(options[0].usi);
    render();
  }
}

function confirmMove(usi) {
  const side = inputStep === 'gote' ? 'gote' : 'sente';
  const text = usiToText(usi, sfens[cursor], side);

  if (onlineMode) {
    // オンラインモード: 自分の陣営だけ確定して commit を送信する
    if (side === 'sente') pendingSente = { usi, text };
    else                  pendingGote  = { usi, text };
    inputStep        = null;
    selectedFrom     = null;
    legalTargets     = null;
    promotionPending = null;
    hidePromotionUI();
    onlineCommitted  = true;
    commitMoveOnline(sfens[cursor], usi);
    return;
  }

  // ホットシートモード（従来）
  if (side === 'sente') {
    pendingSente = { usi, text };
    inputStep    = 'gote';
  } else {
    pendingGote = { usi, text };
  }
  selectedFrom = null; legalTargets = null;
  promotionPending = null; hidePromotionUI();
}

function _resetOnlineState() {
  onlineMode       = false;
  onlineSide       = null;
  onlineGameOver   = false;
  onlineEndMsg     = '';
  onlineCommitted  = false;
  onlineWaiting    = false;
  onlineWaitingMsg = '';
  resultOverride   = null;
  recordInviteAsked = false;
  recordStatusText  = '';
  archivedLink      = null;
  _pendingRecordDisconnect = false;
}

function _onlinePhaseText(gameOver) {
  if (onlineGameOver) {
    if (gameOver || cursor === kifu.plies.length) return onlineEndMsg || gameOver || '終局';
    if (cursor === 0) return '初期局面';
    return `第${cursor}組手後`;
  }
  if (onlineWaiting)   return onlineWaitingMsg;
  if (onlineCommitted) return '着手確定 — 相手の着手を待っています';
  if (onlineSide === 'gote') return selectedFrom ? '後手の手を選択中' : '後手の手を選んでください';
  return selectedFrom ? '先手の手を選択中' : '先手の手を選んでください';
}

function endOnlineGame(msg) {
  onlineGameOver  = true;
  onlineEndMsg    = msg;
  onlineCommitted = false;
  onlineWaiting   = false;
  // 観戦者へライブの終局表示を知らせる（disconnectOnline で ws を閉じる前に
  // 送る必要がある。text は同梱しない——綴じは record_testimony 経路へ移った。
  // 記録係二段目 §10）。
  const result = currentResult();
  sendSpectateResult(result.kind, result.outcome);
  if (isRecording()) {
    // 両陣営が証言として正準本文を送る（§3・§9）。二証人の突き合わせは DO 側の
    // 非同期処理（ハッシュ計算・KV 書き込み）を要するため、すぐ切断すると
    // archived/record_disagreement の通知（§5）を受け取れずに終わる——両者が
    // 証言を送った直後に自分から切断してしまうため。結果が届く（onArchived/
    // onRecordDisagreement）まで、または保険のタイムアウトまで待ってから切断する。
    sendRecordTestimony(result.kind, result.outcome, buildArchiveText());
    _pendingRecordDisconnect = true;
    setTimeout(() => {
      if (_pendingRecordDisconnect) { _pendingRecordDisconnect = false; disconnectOnline(); }
    }, 5000);
  } else {
    // 終局後は WS を閉じる（intentional なので onlineMode は破棄しない）
    disconnectOnline();
  }
  render();
}

function handleTurnComplete(senteUsi, goteUsi) {
  onlineCommitted = false;

  // 投了の検出（ルール 5.3 / 5.4）
  const sResign = senteUsi === 'resign';
  const gResign = goteUsi  === 'resign';
  if (sResign || gResign) {
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
    resultOverride = { kind: 'resign', outcome };
    endOnlineGame(msg);
    return;
  }

  const sText = usiToText(senteUsi, sfens[cursor], 'sente');
  const gText = usiToText(goteUsi,  sfens[cursor], 'gote');
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
  cursor++;
  phase = 'position';
  const msg = getGameOverMsg();
  if (msg) { endOnlineGame(msg); return; }

  if (onlineSide === 'gote') inputStep = 'gote';

  // クリック座標が自分の合法手の駒に当たっていれば選択状態へ直接遷移
  const activeSide = onlineSide === 'gote' ? 'g' : 's';
  const pos = parseSfen(sfens[cursor]);
  const sq  = getBoardSquare(sx, sy);
  if (sq) {
    const [f, r] = sq;
    const piece = pos.board.get(`${f},${r}`);
    if (piece && piece.side === activeSide) selectBoardPiece(f, r);
  } else if (onlineSide === 'gote') {
    const k = getHandPieceAt(pos.handG, 8, sx, sy);
    if (k) selectHandPiece(k);
  } else {
    const k = getHandPieceAt(pos.handS, BY + BH + 12, sx, sy);
    if (k) selectHandPiece(k);
  }
  render();
}

function handleSvgClick(event) {
  if (watchMode) return;  // 観戦は読み取り専用（盤クリックで着手できない）

  // 同時開示フェーズ: 盤面・駒台クリックで次局面へ遷移
  if (phase === 'reveal' && onlineMode && !onlineGameOver) {
    const { x: sx, y: sy } = svgCoords(event);
    _advanceFromReveal(sx, sy);
    return;
  }

  if (phase !== 'position') return;
  if (promotionPending)     return;
  if (onlineMode && onlineCommitted) return;

  const { x: sx, y: sy } = svgCoords(event);
  const gameOver = getGameOverMsg();
  const pos      = parseSfen(sfens[cursor]);
  const activeSide = inputStep === 'gote' ? 'g' : 's';

  // If target selection is active, check for legal target click first
  if (legalTargets) {
    const sq = getBoardSquare(sx, sy);
    if (sq) {
      const key = `${sq[0]},${sq[1]}`;
      if (legalTargets.has(key)) { selectTarget(sq[0], sq[1]); render(); return; }
    }
  }

  // Clicks disabled when game is over and no input is active
  if (gameOver && !inputStep) return;

  // Board square click
  const sq = getBoardSquare(sx, sy);
  if (sq) {
    const [f, r] = sq;
    const piece  = pos.board.get(`${f},${r}`);
    if (piece && piece.side === activeSide) {
      // Toggle selection on same piece; switch to different own piece
      if (selectedFrom?.board?.[0] === f && selectedFrom?.board?.[1] === r) {
        selectedFrom = null; legalTargets = null;
      } else {
        selectBoardPiece(f, r);
      }
      render(); return;
    }
    // Clicked empty or opponent square → deselect without changing inputStep
    if (selectedFrom) { selectedFrom = null; legalTargets = null; render(); }
    return;
  }

  // Gote hand (y=8) — only during gote's turn
  if (inputStep === 'gote') {
    const k = getHandPieceAt(pos.handG, 8, sx, sy);
    if (k) {
      if (selectedFrom?.hand === k) { selectedFrom = null; legalTargets = null; }
      else selectHandPiece(k);
      render(); return;
    }
  }

  // Sente hand (y=BY+BH+12) — during sente's turn or before input starts
  if (inputStep !== 'gote') {
    const k = getHandPieceAt(pos.handS, BY + BH + 12, sx, sy);
    if (k) {
      if (selectedFrom?.hand === k) { selectedFrom = null; legalTargets = null; }
      else selectHandPiece(k);
      render(); return;
    }
  }
}

// ── Overlay computation ────────────────────────────────────────────────────────

function computeRevealOverlay() {
  const ply = kifu.plies[cursor];
  if (!ply) return null;
  const s = parseUsi(ply.sUsi);
  const g = parseUsi(ply.gUsi);
  return {
    board:         [s.isDrop ? null : s.from, s.to, g.isDrop ? null : g.from, g.to].filter(Boolean),
    sHand:         s.isDrop ? new Set([s.kind]) : null,
    gHand:         g.isDrop ? new Set([g.kind]) : null,
    legalDots:     null,
    selectedSquare: null,
  };
}

function computeInputOverlay() {
  const overlay = { board: [], sHand: null, gHand: null, legalDots: null, selectedSquare: null };

  if (selectedFrom?.board) {
    overlay.selectedSquare = selectedFrom.board;
  } else if (selectedFrom?.hand) {
    if (inputStep === 'gote') {
      overlay.gHand = overlay.gHand || new Set();
      overlay.gHand.add(selectedFrom.hand);
    } else {
      overlay.sHand = overlay.sHand || new Set();
      overlay.sHand.add(selectedFrom.hand);
    }
  }

  if (legalTargets) {
    overlay.legalDots = new Set(legalTargets.keys());
  }

  return overlay;
}

// ── Navigation ────────────────────────────────────────────────────────────────

function goNext() {
  if (promotionPending) return;
  if (onlineMode && !onlineGameOver) return; // 対局中はナビ不可

  if (pendingSente && pendingGote) {
    branchAndAppend(pendingSente.usi, pendingGote.usi, pendingSente.text, pendingGote.text);
    render(); return;
  }

  if (phase === 'position' && cursor < kifu.plies.length) {
    phase = 'reveal';
  } else if (phase === 'reveal') {
    cursor++;
    phase = 'position';
  }
  render();
}

function goPrev() {
  if (onlineMode && !onlineGameOver) return; // 対局中はナビ不可
  if (promotionPending) {
    promotionPending = null; hidePromotionUI();
    selectedFrom = null; legalTargets = null;
    render(); return;
  }

  if (inputStep !== null || selectedFrom !== null) {
    // One press cancels all input state; second press starts navigating back
    resetInput();
    render(); return;
  }

  if (phase === 'reveal') {
    phase = 'position';
  } else if (phase === 'position' && cursor > 0) {
    cursor--;
    phase = 'reveal';
  }
  render();
}

// ── Render ────────────────────────────────────────────────────────────────────

function render() {
  const pos       = parseSfen(sfens[cursor]);
  const bothReady = !!(pendingSente && pendingGote);
  const hasInput  = !!(inputStep || selectedFrom || pendingSente || pendingGote);
  const gameOver  = getGameOverMsg();

  let overlay, moveText = '', phaseText = '', eventText = '';

  if (phase === 'reveal') {
    overlay   = computeRevealOverlay();
    const ply = kifu.plies[cursor];
    moveText  = `${ply.sText}　${ply.gText}`;
    phaseText = '同時開示';
    const evKey = events[cursor];
    eventText = (evKey && evKey !== 'normal') ? `（${EVENT_LABEL[evKey] || evKey}）` : '';
  } else {
    overlay = hasInput ? computeInputOverlay() : null;

    if (watchMode) {
      phaseText = _watchPhaseText(gameOver);
    } else if (onlineMode) {
      phaseText = _onlinePhaseText(gameOver);
      if (!onlineGameOver && onlineCommitted) {
        moveText = onlineSide === 'sente' ? (pendingSente?.text || '') : (pendingGote?.text || '');
      }
    } else if (bothReady) {
      moveText  = `${pendingSente.text}　${pendingGote.text}`;
      phaseText = '解決してください';
    } else if (pendingSente) {
      moveText  = pendingSente.text;
      phaseText = '後手の手を選択中';
    } else if (inputStep === 'gote') {
      phaseText = '後手の手を選択中';
    } else if (inputStep === 'sente' || selectedFrom) {
      phaseText = '先手の手を選択中';
    } else if (gameOver) {
      phaseText = gameOver;
    } else if (cursor === 0) {
      phaseText = '初期局面';
    } else {
      phaseText = `第${cursor}組手後`;
    }
  }

  const svg = document.getElementById('board');
  svg.setAttribute('viewBox', `0 0 ${SVG_W} ${SVG_H}`);
  svg.innerHTML = renderSvg(pos, overlay);
  svg.style.cursor = (phase === 'position' && !gameOver && !watchMode && !(onlineMode && onlineCommitted))
    ? 'pointer' : 'default';

  document.getElementById('phase-label').textContent  = phaseText;
  document.getElementById('move-display').textContent = moveText;
  document.getElementById('event-label').textContent  = eventText || ' ';

  const archiveInfo = archiveInfoText();
  const archiveInfoEl = document.getElementById('archive-info');
  archiveInfoEl.textContent = archiveInfo.text;
  archiveInfoEl.classList.toggle('mismatch', archiveInfo.mismatch);

  const total = kifu.plies.length * 2 + 1;
  const step  = cursor * 2 + (phase === 'reveal' ? 1 : 0) + 1;
  document.getElementById('step-label').textContent = `${step} / ${total}`;

  const btnNext = document.getElementById('btn-next');
  const btnPrev = document.getElementById('btn-prev');

  if (watchMode) {
    // 観戦中は常に棋譜ナビゲーション可能（コミット待ちの概念が無い）。
    btnNext.textContent = '次 →';
    btnNext.disabled = !(
      phase === 'reveal' ||
      (phase === 'position' && cursor < kifu.plies.length)
    );
    btnPrev.disabled = cursor === 0 && phase === 'position';
  } else if (onlineMode) {
    btnNext.textContent = '次 →';
    if (onlineGameOver) {
      // 終局後は棋譜ナビゲーションを解放（phase に関係なく維持）
      btnNext.disabled = !(
        phase === 'reveal' ||
        (phase === 'position' && cursor < kifu.plies.length)
      );
      btnPrev.disabled = cursor === 0 && phase === 'position';
    } else {
      btnNext.disabled = true;
      btnPrev.disabled = true;
    }
  } else {
    btnNext.textContent = bothReady ? '解決 →' : '次 →';
    btnNext.disabled    = !(
      bothReady ||
      phase === 'reveal' ||
      (phase === 'position' && !hasInput && cursor < kifu.plies.length)
    );
    btnPrev.disabled    = (
      cursor === 0 && phase === 'position' && !hasInput && !promotionPending
    );
  }

  const btnResign = document.getElementById('btn-resign');
  if (btnResign) {
    btnResign.style.display = (onlineMode && !onlineGameOver) ? 'inline-block' : 'none';
    btnResign.disabled      = onlineCommitted || onlineWaiting;
  }

  const btnSave = document.getElementById('btn-save');
  if (btnSave) {
    const isOver = onlineMode ? onlineGameOver : !!gameOver;
    btnSave.classList.toggle('highlight', isOver);
  }

  // 観戦中は対局を始める系のボタンを封じ、代わりに「観戦をやめる」を出す。
  for (const id of ['btn-online', 'btn-load']) {
    const btn = document.getElementById(id);
    if (btn) btn.disabled = watchMode;
  }
  const btnLeaveWatch = document.getElementById('btn-leave-watch');
  if (btnLeaveWatch) btnLeaveWatch.hidden = !watchMode;

  const linkText = document.getElementById('watch-link-text');
  const linkBtn  = document.getElementById('btn-copy-watch-link');
  if (linkText && linkBtn) {
    if (onlineMode && spectateToken) {
      const link = `${location.origin}${location.pathname}?watch=${encodeURIComponent(spectateToken)}`;
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
    recordText.textContent = recordStatusText;
    if (archivedLink) {
      recordBtn.hidden = false;
      recordBtn.dataset.link = archivedLink.url;
    } else {
      recordBtn.hidden = true;
    }
  }
}

function _watchPhaseText(gameOver) {
  if (watchStatusText === 'connecting') return '観戦: 接続中…';
  if (watchStatusText === 'error')      return '観戦: 接続エラーが発生しました';
  if (watchStatusText === 'closed')     return '観戦: 接続が切れました';

  // 投了など盤面から導けない終局は result で判断する（player_disconnected は
  // 対局終了時の意図した WS 切断でも届くため、既に終局済みなら「再接続待ち」
  // という誤解を招く表示にしない）。
  const concluded = !!(loadedMeta?.result && loadedMeta.result.kind !== 'unfinished');
  if (watchStatusText === 'player_disconnected' && !concluded) {
    return '観戦: プレイヤーが切断中です（再接続を待っています）';
  }
  if (concluded && cursor === kifu.plies.length) return formatResult(loadedMeta.result);
  if (gameOver) return gameOver;
  if (kifu.plies.length === 0) return '観戦中（開始を待っています）';
  if (cursor === kifu.plies.length) return '観戦中（最新）';
  return `観戦中（第${cursor}組手）`;
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
    render();
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
    if (!promotionPending) return;
    const usi = promotionPending.options.find(o => o.promote)?.usi;
    if (usi) { confirmMove(usi); render(); }
  });
  document.getElementById('btn-no-promote').addEventListener('click', () => {
    if (!promotionPending) return;
    const usi = promotionPending.options.find(o => !o.promote)?.usi;
    if (usi) { confirmMove(usi); render(); }
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
      if (!onlineGameOver) {
        disconnectOnline();
        if (onlineMode) { _resetOnlineState(); resetToNew(); }
      }
      modal.classList.remove('visible');
      statusEl.textContent = '—';
      btnConn.disabled = false;
      btnConn.textContent = '入室';
      render();
    };

    document.getElementById('btn-resign').addEventListener('click', () => {
      if (!onlineMode || onlineGameOver || onlineCommitted) return;
      if (!confirm('投了しますか？')) return;
      // 投了は commit-reveal プロトコル経由。即終局にしない（両者投了の引き分けを拾うため）
      commitMoveOnline(sfens[cursor], 'resign');
      onlineCommitted = true;
      render();
    });

    document.getElementById('btn-online').addEventListener('click', () => {
      // 前回の対局が終局済みなら畳んでから新しい接続へ（「新局」ボタンが担っていた役割）
      if (onlineGameOver) { _resetOnlineState(); resetToNew(); }
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
        onStatus: (state, msg) => {
          statusEl.textContent = msg;

          if (state === 'ready') {
            if (!onlineMode) {
              // 初回接続: オンラインモード開始
              onlineMode       = true;
              onlineSide       = getMySide();
              onlineCommitted  = false;
              onlineGameOver   = false;
              onlineEndMsg     = '';
              onlineWaiting    = false;
              onlineWaitingMsg = '';
              spectateToken    = null;
              resetToNew();
              if (onlineSide === 'gote') inputStep = 'gote';
              sendSpectateMeta(versionTuple, sfens[0]);
            } else {
              // 再接続完了: ゲーム状態はそのまま、waiting 解除
              onlineWaiting    = false;
              onlineWaitingMsg = '';
              onlineCommitted  = false;
            }
            modal.classList.remove('visible');
            btnConn.disabled = false;
            btnConn.textContent = '入室';
            render();

            // 記録係への招待の prompt（対局開始・握手完了時。記録係二段目 §5）。
            // モーダルが閉じて盤面が見えた後に出す。招き忘れ対策——オプトイン
            // だが必ず尋ねる。相手も同時に自分の招待を出しうる（どちらから
            // 提案してもよい。§2）ので、二重の招待が交差しても害はない。
            if (!recordInviteAsked) {
              recordInviteAsked = true;
              setTimeout(() => {
                if (confirm('記録係をこの対局に招いて綴じてもらいますか？（相手の同意が必要です）')) {
                  sendRecordInvite();
                }
              }, 0);
            }

          } else if (state === 'peer_disconnected') {
            // 相手が切断: ゲーム状態維持、待機表示
            onlineWaiting    = true;
            onlineWaitingMsg = msg;
            onlineCommitted  = false;
            render();

          } else if (state === 'self_disconnected') {
            // 自分が切断: 再接続可能な状態で待機
            onlineWaiting    = true;
            onlineWaitingMsg = msg;
            onlineCommitted  = false;
            btnConn.disabled = false;
            btnConn.textContent = '再接続';
            render();

          } else if (state === 'error') {
            if (onlineMode && !onlineGameOver) {
              onlineWaiting    = true;
              onlineWaitingMsg = `エラー: ${msg}`;
            }
            btnConn.disabled = false;
            btnConn.textContent = '入室';
            render();

          } else if (state === 'disconnected') {
            if (!onlineGameOver) _resetOnlineState();
            btnConn.disabled = false;
            btnConn.textContent = '入室';
            render();
          }
        },
        onTurnComplete:  handleTurnComplete,
        onPeerAborted:   (reason) => endOnlineGame(`中断: ${reason}`),
        onSpectateToken: (token) => { spectateToken = token; render(); },
        onRecordInvite: () => {
          // 相手からの記録係への招待提案（記録係二段目 §2・§5）。
          if (confirm('相手が記録係をこの対局に招いて綴じることを提案しました。同意しますか？')) {
            sendRecordAccept();
          } else {
            sendRecordDecline();
          }
        },
        onRecordConfirmed: () => {
          recordStatusText = '記録係: 有効（この対局は書庫へ綴じられます）';
          render();
        },
        onRecordDeclined: () => {
          recordStatusText = '';
          alert('相手が記録を辞退しました。この対局は綴じられません。');
          render();
        },
        onRecordDisagreement: (idA, idB, id) => {
          recordStatusText = '記録が食い違いました（裁定はされません）';
          archivedLink = id ? { id, url: archiveUrl(id) } : null;
          alert('二人の証言が一致しませんでした。改竄検知として記録し、裁定はしません。');
          if (_pendingRecordDisconnect) { _pendingRecordDisconnect = false; disconnectOnline(); }
          render();
        },
        onArchived: (id) => {
          recordStatusText = '記録されました';
          archivedLink = { id, url: archiveUrl(id) };
          if (_pendingRecordDisconnect) { _pendingRecordDisconnect = false; disconnectOnline(); }
          render();
        },
        getSfens:        () => sfens,
        onResumeAt:      (resumeSfen) => {
          const idx = sfens.indexOf(resumeSfen);
          if (idx >= 0) {
            cursor           = idx;
            phase            = 'position';
            onlineWaiting    = false;
            onlineWaitingMsg = '';
            onlineCommitted  = false;
            resetInput();  // selectedFrom・legalTargets 等をクリア（inputStep は null になる）
            inputStep = onlineSide === 'gote' ? 'gote' : 'sente';
          }
          render();
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
    versionTuple = JSON.parse(wasmVersionTuple());
    maxTurns = wasmMaxTurns();

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
