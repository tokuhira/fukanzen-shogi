/**
 * online.js — WebSocket 殻と ProtocolSession を繋ぐディスパッチループ。
 *
 * 接続フロー:
 *   connectOnline(roomKey, secret, onStatus) を呼ぶ
 *   → WebSocket 接続 → DO から peer_joined/room_ready で陣営決定
 *   → hello_msg 送信 → 相手の hello を feed() → handshake_done
 *
 * I/O（WebSocket の送受信）はここで担い、
 * ゲームの判定はすべて ProtocolSession（Wasm）が担う。
 */

import initProtocol, { ProtocolSession } from './protocol-wasm/protocol_wasm.js';

// 本番 Workers URL。ローカル確認時は wrangler dev の URL に変更する。
const WS_BASE_URL = 'wss://fukanzen-shogi-ws.tokuhira.workers.dev';

let ws      = null;
let session = null;

// ── 公開 API ─────────────────────────────────────────────────────────────────

/**
 * ルームへ接続する。
 * @param {string}   roomKey  ルームキー（合言葉）
 * @param {string}   secret   共有パスワード
 * @param {Function} onStatus (state, msg) コールバック
 *   state: 'waiting'|'handshaking'|'ready'|'disconnected'|'error'
 */
export async function connectOnline(roomKey, secret, onStatus) {
  // 既存接続を切る
  if (ws) { ws.close(); ws = null; session = null; }

  // protocol-wasm Wasm を初期化（2 回目以降は no-op）
  await initProtocol();

  ws = new WebSocket(`${WS_BASE_URL}/room/${encodeURIComponent(roomKey)}`);

  ws.addEventListener('open', () => {
    onStatus('waiting', '相手の入室を待っています…');
  });

  ws.addEventListener('close', () => {
    onStatus('disconnected', '切断されました');
    session = null;
  });

  ws.addEventListener('error', () => {
    onStatus('error', '接続エラー');
    session = null;
  });

  ws.addEventListener('message', (evt) => {
    _handleMessage(evt.data, secret, onStatus);
  });
}

/** 接続を切断してセッションを破棄する。 */
export function disconnectOnline() {
  ws?.close();
  ws = null;
  session = null;
}

/** 現在の ProtocolSession を返す（Step D 以降で使用）。 */
export const getSession = () => session;

/** 現在の WebSocket を返す（Step D 以降で使用）。 */
export const getWs = () => ws;

// ── 受信ディスパッチ ──────────────────────────────────────────────────────────

function _handleMessage(data, secret, onStatus) {
  let msg;
  try { msg = JSON.parse(data); } catch { return; }

  // DO から来るシステムメッセージ（ゲームと無関係な制御情報）
  if (msg.type === 'peer_joined' || msg.type === 'room_ready') {
    // 陣営決定 → ProtocolSession 生成 → hello 送信
    const mySide = msg.your_side;             // "sente" | "gote"
    session = new ProtocolSession(mySide, secret);
    ws.send(session.hello_msg());
    const label = mySide === 'sente' ? '先手' : '後手';
    onStatus('handshaking', `握手中 (${label})…`);
    return;
  }

  if (msg.type === 'peer_disconnected') {
    onStatus('disconnected', '相手が切断しました');
    session = null;
    return;
  }

  // 以下は ProtocolSession へ転送
  if (!session) return;

  let result;
  try { result = JSON.parse(session.feed(data)); } catch { return; }

  if (!result.ok) {
    onStatus('error', `プロトコルエラー: ${result.error}`);
    ws.close();
    return;
  }

  switch (result.event) {
    case 'handshake_done': {
      const peerLabel = result.peer_side === 'sente' ? '先手' : '後手';
      onStatus('ready', `握手完了 — 相手は${peerLabel}`);
      break;
    }
    // Step D 以降のイベント（commit/reveal/ack/turn_complete）はここで捌く
    default:
      break;
  }
}
