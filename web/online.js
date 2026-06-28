/**
 * online.js — WebSocket 殻と ProtocolSession を繋ぐディスパッチループ。
 *
 * 接続フロー:
 *   connectOnline(roomKey, secret, callbacks) を呼ぶ
 *   → WS 接続 → peer_joined/room_ready で陣営決定 → hello 送信 → handshake_done
 *
 * 一手フロー（Step D）:
 *   board.js が commitMoveOnline(sfen, usi) を呼ぶ
 *   → commit 送信 → 両者 commit 揃い次第 reveal 送信（自動）
 *   → peer_reveal 検証後 ack 送信（自動）
 *   → peer_ack 受信 → callbacks.onTurnComplete(senteUsi, goteUsi) 呼び出し
 *
 * 切断・再接続フロー（Step F）:
 *   peer 切断 → onStatus('peer_disconnected', ...) → 相手の再接続を待機
 *   自分切断 → セッション保持・onStatus('self_disconnected', ...) → reconnectOnline() で再接続
 *   再接続時: you_reconnected → reconnect メッセージ送信 → reconnect_ack で再開
 *
 * I/O（WebSocket 送受信）はここで担い、ゲームの判定は ProtocolSession (Wasm) が担う。
 */

import initProtocol, { ProtocolSession, sfen_hash as sfenHash } from './protocol-wasm/protocol_wasm.js';

// 本番 Workers URL。ローカル確認時は wrangler dev の URL に変更する。
const WS_BASE_URL = 'wss://fukanzen-shogi-ws.tokuhira.workers.dev';

// ── モジュールスコープ変数 ────────────────────────────────────────────────────

let ws       = null;
let session  = null;
let mySide   = null;   // 'sente' | 'gote'（陣営決定後に確定）

// 再接続のためにルームキー・シークレットを保持
let _roomKey = null;
let _secret  = null;

// 意図的切断フラグ（disconnectOnline() / endGame 後の WS close を区別する）
let _intentionalDisconnect = false;

// request_reset 後の自動リトライ制御
let _pendingReset  = false;   // true のとき _onWsClose で自動再接続する
let _resetAttempts = 0;       // 連続リトライ回数（無限ループ防止）

// 一手あたりのターン状態
let myCommitted = false;  // commit 送信済み
let revealSent  = false;  // reveal 送信済み

// コールバック
let _cbs = null;
// { onStatus, onTurnComplete, onPeerAborted, getSfens, onResumeAt }
//   onStatus(state, msg):
//     'waiting'|'handshaking'|'ready'|'disconnected'|'error'
//     'peer_disconnected'|'self_disconnected'|'peer_reconnecting'|'reconnected'
//   onTurnComplete(senteUsi, goteUsi)
//   onPeerAborted(reason)
//   getSfens() → string[]  ← 再接続時の盤面ハッシュ照合に使う
//   onResumeAt(sfen)       ← 再接続後の再開局面を board.js へ通知する

// ── 公開 API ─────────────────────────────────────────────────────────────────

/**
 * ルームへ接続する（新規ゲーム）。
 * 既存セッションがある場合は破棄して新規接続する。
 */
export async function connectOnline(roomKey, secret, callbacks) {
  _cbs           = callbacks;
  _roomKey       = roomKey;
  _secret        = secret;
  _resetAttempts = 0;

  if (ws) {
    _intentionalDisconnect = true;
    ws.close();
    ws = null;
  }
  session = null;
  mySide  = null;
  _resetTurnState();

  await initProtocol();

  _openWs();
}

function _openWs(onOpen = () => _cbs?.onStatus('waiting', '相手の入室を待っています…')) {
  ws = new WebSocket(`${WS_BASE_URL}/room/${encodeURIComponent(_roomKey)}`);
  ws.addEventListener('open',    onOpen);
  ws.addEventListener('close',   _onWsClose);
  ws.addEventListener('error',   () => _cbs?.onStatus('error', '接続エラー'));
  ws.addEventListener('message', (evt) => _handleMessage(evt.data));
}

/**
 * 切断後にセッションを維持したまま WebSocket だけ再接続する。
 * connectOnline() とは異なり、session・mySide は破棄しない。
 * you_reconnected が届いたら reconnect メッセージを自動送信する。
 */
export async function reconnectOnline() {
  if (!_roomKey || !session) return;
  _openWs(() => _cbs?.onStatus('handshaking', '再接続中…'));
}

/** 接続を切断してセッションを完全に破棄する。 */
export function disconnectOnline() {
  _intentionalDisconnect = true;
  if (ws) ws.close();
  ws      = null;
  session = null;
  mySide  = null;
  _resetTurnState();
}

/**
 * 自分の着手を commit して送信する。board.js が呼ぶ。
 * @param {string} sfen  現在局面の SFEN
 * @param {string} usi   着手の USI 表記（"resign" を含む）
 */
export async function commitMoveOnline(sfen, usi) {
  if (!session || !ws) return;

  const result = JSON.parse(session.commit_move(sfen, usi));
  if (!result.ok) {
    _cbs?.onStatus('error', `commit エラー: ${result.error}`);
    return;
  }

  ws.send(JSON.stringify(result.message));
  myCommitted = true;

  if (result.both_committed) {
    // peer commit がバッファ済みだった → 即 reveal
    _sendReveal();
  } else {
    _cbs?.onStatus('handshaking', '着手確定 — 相手の着手を待っています');
  }
}

/** 現在の陣営（'sente'|'gote'|null）を返す。 */
export const getMySide = () => mySide;

/** 接続中かどうか。 */
export const isOnline = () => ws !== null && session !== null;

/** セッションが生きている（切断後の再接続が可能な）状態かどうか。 */
export const hasReconnectableSession = () => session !== null && ws === null;

// ── WS close ハンドラ ─────────────────────────────────────────────────────────

function _onWsClose() {
  const intentional  = _intentionalDisconnect;
  const pendingReset = _pendingReset;
  _intentionalDisconnect = false;
  _pendingReset          = false;
  ws = null;
  _resetTurnState();

  if (pendingReset) {
    // request_reset 後の自動リトライ: ユーザーにエラーを見せずに再接続する
    setTimeout(_openWs, 600);
    return;
  }

  if (intentional) {
    _cbs?.onStatus('disconnected', '切断されました');
  } else {
    // 予期せぬ切断: session を保持して再接続を待つ
    _cbs?.onStatus('self_disconnected', '接続が切れました。再接続できます。');
  }
}

// ── 受信ディスパッチ ──────────────────────────────────────────────────────────

function _handleMessage(data) {
  if (!ws) return;

  let msg;
  try { msg = JSON.parse(data); } catch { return; }

  // ── DO からのシステムメッセージ ───────────────────────────────────────────

  if (msg.type === 'peer_joined' || msg.type === 'room_ready') {
    mySide  = msg.your_side;
    session = new ProtocolSession(mySide, _secret);
    ws.send(session.hello_msg());
    const label = mySide === 'sente' ? '先手' : '後手';
    _cbs?.onStatus('handshaking', `握手中 (${label})…`);
    return;
  }

  if (msg.type === 'peer_disconnected') {
    if (!session) {
      // 対局開始前に相手が切断 → 自分の WS も閉じてサーバー側リセットをトリガー
      if (ws) { ws.close(); ws = null; }
      return;
    }
    // 相手切断: ゲーム状態を維持して再接続を待つ
    _resetTurnState();
    _cbs?.onStatus('peer_disconnected', '相手が切断しました。再接続を待っています…');
    return;
  }

  if (msg.type === 'you_reconnected') {
    if (!session) {
      // session なしで再接続フロー受信 = stale gameStarted + zombie WS
      // 3回まで request_reset → 自動リトライ。超えたらエラーで止める
      _resetAttempts++;
      if (_resetAttempts > 3) {
        _cbs?.onStatus('error', '入室できません。ページをリロードしてください。');
        if (ws) { _intentionalDisconnect = true; ws.close(); ws = null; }
        return;
      }
      if (ws) {
        _pendingReset = true;
        try { ws.send(JSON.stringify({ type: 'request_reset' })); } catch {}
        ws.close();
        ws = null;
      }
      return;
    }
    // 自分が再接続プレイヤー: reconnect メッセージを送信
    const sfens = _cbs?.getSfens?.() ?? [];
    const currentSfen = sfens[sfens.length - 1] ?? '';
    const hash = currentSfen ? sfenHash(currentSfen) : '';
    if (!hash) {
      _cbs?.onStatus('error', '再接続: 局面ハッシュを計算できません');
      return;
    }
    const result = JSON.parse(session.reconnect_msg(hash));
    if (result.ok) {
      ws.send(JSON.stringify(result.message));
      _cbs?.onStatus('handshaking', '再接続中 — 相手の認証を待っています…');
    }
    return;
  }

  if (msg.type === 'peer_reconnected') {
    // 相手が再接続: reconnect メッセージを待つ
    _cbs?.onStatus('handshaking', '相手が再接続しました。認証中…');
    return;
  }

  // ── ProtocolSession へ転送 ───────────────────────────────────────────────
  if (!session) return;

  let result;
  try { result = JSON.parse(session.feed(data)); } catch { return; }

  if (!result.ok) {
    _cbs?.onStatus('error', `プロトコルエラー: ${result.error}`);
    ws.close();
    return;
  }

  switch (result.event) {

    case 'handshake_done': {
      const peerLabel = result.peer_side === 'sente' ? '先手' : '後手';
      _cbs?.onStatus('ready', `握手完了 — 相手は${peerLabel}`);
      break;
    }

    case 'peer_committed': {
      if (result.both_committed) {
        if (myCommitted && !revealSent) _sendReveal();
      }
      break;
    }

    case 'peer_commit_buffered':
      break;

    case 'peer_revealed': {
      if (result.both_revealed) {
        const ackResult = JSON.parse(session.ack_msg());
        if (ackResult.ok) {
          ws.send(JSON.stringify(ackResult.message));
          _cbs?.onStatus('handshaking', '開示完了 — Ack 送受信中');
        } else {
          _cbs?.onStatus('error', `ack エラー: ${ackResult.error}`);
        }
      }
      break;
    }

    case 'turn_complete': {
      _completeTurn(result.sente_usi, result.gote_usi);
      break;
    }

    case 'peer_acked':
      break;

    case 'peer_aborted': {
      _resetTurnState();
      _cbs?.onPeerAborted?.(result.reason);
      break;
    }

    // ── 再接続プロトコル ──────────────────────────────────────────────────

    case 'peer_reconnect_request': {
      // 残留プレイヤー側: 相手の reconnect を検証し ack を返す
      const expectedAuthHash = session.peer_auth_hash();
      if (!expectedAuthHash || result.auth_hash !== expectedAuthHash) {
        ws.send(JSON.stringify({ type: 'abort', reason: 'auth_mismatch' }));
        _cbs?.onStatus('error', '再接続: 認証失敗');
        return;
      }
      const resumeSfen = _findSfenByHash(result.board_hash);
      if (!resumeSfen) {
        ws.send(JSON.stringify({ type: 'abort', reason: 'hash_mismatch' }));
        _cbs?.onStatus('error', '再接続: 棋譜が一致しません');
        return;
      }
      ws.send(JSON.stringify({ type: 'reconnect_ack', board_hash: result.board_hash }));
      _cbs?.onStatus('ready', '対局中');
      _cbs?.onResumeAt?.(resumeSfen);
      break;
    }

    case 'reconnect_ack': {
      // 再接続プレイヤー側: 相手から承認を受けて再開点を特定
      const resumeSfen = _findSfenByHash(result.resume_hash);
      if (!resumeSfen) {
        _cbs?.onStatus('error', '再接続: 再開局面が見つかりません');
        return;
      }
      _cbs?.onStatus('ready', '対局中');
      _cbs?.onResumeAt?.(resumeSfen);
      break;
    }
  }
}

// ── 内部ヘルパー ──────────────────────────────────────────────────────────────

function _findSfenByHash(hash) {
  const sfens = _cbs?.getSfens?.() ?? [];
  return sfens.find(sfen => sfenHash(sfen) === hash) ?? null;
}

function _sendReveal() {
  if (!session || revealSent) return;
  const result = JSON.parse(session.reveal_msg());
  if (result.ok) {
    ws.send(JSON.stringify(result.message));
    revealSent = true;
    _cbs?.onStatus('handshaking', '開示済み — 相手の開示を待っています');
  } else {
    _cbs?.onStatus('error', `reveal エラー: ${result.error}`);
  }
}

function _completeTurn(senteUsi, goteUsi) {
  _resetTurnState();
  _cbs?.onTurnComplete?.(senteUsi, goteUsi);
  _cbs?.onStatus('ready', '対局中');
}

function _resetTurnState() {
  myCommitted = false;
  revealSent  = false;
}
