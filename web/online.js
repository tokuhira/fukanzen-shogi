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
 * I/O（WebSocket 送受信）はここで担い、ゲームの判定は ProtocolSession (Wasm) が担う。
 */

import initProtocol, { ProtocolSession } from './protocol-wasm/protocol_wasm.js';

// 本番 Workers URL。ローカル確認時は wrangler dev の URL に変更する。
const WS_BASE_URL = 'wss://fukanzen-shogi-ws.tokuhira.workers.dev';

// ── モジュールスコープ変数 ────────────────────────────────────────────────────

let ws       = null;
let session  = null;
let mySide   = null;   // 'sente' | 'gote'（陣営決定後に確定）

// 一手あたりのターン状態
let myCommitted = false;  // commit 送信済み
let revealSent  = false;  // reveal 送信済み

// 終局フラグ（投了・被投了後はプロトコルループを止める）
let _gameEnded = false;

// コールバック
let _cbs = null; // { onStatus, onTurnComplete, onPeerAborted }

// ── 公開 API ─────────────────────────────────────────────────────────────────

/**
 * ルームへ接続する。
 * @param {string}   roomKey
 * @param {string}   secret   共有パスワード
 * @param {{ onStatus, onTurnComplete }} callbacks
 *   onStatus(state, msg) — state: 'waiting'|'handshaking'|'ready'|'disconnected'|'error'
 *   onTurnComplete(senteUsi, goteUsi) — ターン確定時に board.js が受け取る
 */
export async function connectOnline(roomKey, secret, callbacks) {
  _cbs = callbacks;

  if (ws) { ws.close(); ws = null; session = null; }
  _resetTurnState();

  await initProtocol();

  ws = new WebSocket(`${WS_BASE_URL}/room/${encodeURIComponent(roomKey)}`);

  ws.addEventListener('open',    () => _cbs?.onStatus('waiting', '相手の入室を待っています…'));
  ws.addEventListener('close',   () => { _cbs?.onStatus('disconnected', '切断されました'); session = null; });
  ws.addEventListener('error',   () => { _cbs?.onStatus('error', '接続エラー');            session = null; });
  ws.addEventListener('message', (evt) => _handleMessage(evt.data, secret));
}

/** 接続を切断してセッションを破棄する。 */
export function disconnectOnline() {
  ws?.close();
  ws = null;
  session = null;
  mySide = null;
  _gameEnded = false;
  _resetTurnState();
}

/**
 * 投了メッセージを送信して終局にする。
 * board.js が呼ぶ。
 */
export function resignOnline() {
  if (!ws || !session || _gameEnded) return;
  _gameEnded = true;
  ws.send(JSON.stringify({ type: 'abort', reason: 'resign' }));
}

/**
 * 自分の着手を commit して送信する。board.js が呼ぶ。
 * @param {string} sfen  現在局面の SFEN
 * @param {string} usi   着手の USI 表記
 */
export async function commitMoveOnline(sfen, usi) {
  if (!session || !ws || _gameEnded) return;

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

// ── 受信ディスパッチ ──────────────────────────────────────────────────────────

function _handleMessage(data, secret) {
  let msg;
  try { msg = JSON.parse(data); } catch { return; }

  // ── DO からのシステムメッセージ ───────────────────────────────────────────
  if (msg.type === 'peer_joined' || msg.type === 'room_ready') {
    mySide  = msg.your_side;
    session = new ProtocolSession(mySide, secret);
    ws.send(session.hello_msg());
    const label = mySide === 'sente' ? '先手' : '後手';
    _cbs?.onStatus('handshaking', `握手中 (${label})…`);
    return;
  }

  if (msg.type === 'peer_disconnected') {
    _cbs?.onStatus('disconnected', '相手が切断しました');
    session = null;
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
        // 両者 commit 確定 → reveal
        if (myCommitted && !revealSent) _sendReveal();
      }
      // myCommitted が false の場合: peer commit はバッファ済み。
      // commitMoveOnline() が後から呼ばれたとき both_committed が true になる。
      break;
    }

    case 'peer_commit_buffered':
      // 自分の commit より先に届いた。commitMoveOnline() 内で自動適用される。
      break;

    case 'peer_revealed': {
      if (result.both_revealed) {
        // reveal 照合完了 → ack 送信
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
      // peer の ack を受信済みだが、まだ自分の ack 前 — 通常は起きないが念のため無視
      break;

    case 'peer_aborted': {
      _gameEnded = true;
      _resetTurnState();
      _cbs?.onPeerAborted?.(result.reason);
      break;
    }
  }
}

// ── 内部ヘルパー ──────────────────────────────────────────────────────────────

function _sendReveal() {
  if (!session || revealSent || _gameEnded) return;
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
  // onTurnComplete の後に onStatus('ready') を呼ぶと
  // board.js 側の再レンダー後に上書きされるため最後に呼ぶ
  _cbs?.onStatus('ready', '対局中');
}

function _resetTurnState() {
  myCommitted = false;
  revealSent  = false;
}
