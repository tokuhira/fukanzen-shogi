// GameRoom（room.ts）の DO 状態から切り離せる純粋ロジック。
// テスト可能にするための最小限の抽出（足場の整備 §3）。
// DO の副作用（storage/KV I/O・WebSocket 送受信）は room.ts 側に残す。
// このモジュール自体は挙動を変えない——room.ts に元々あった判定をそのまま移しただけ。

export const MAX_TURNS = 500; // 出典: engine::terminate::MAX_TURNS（ルール v0.6 の最長手数）
export const MAX_ARCHIVE_TEXT_BYTES = 512 * 1024; // 出典: web/board.js の MAX_ARCHIVE_BYTES

export interface SpectateTurn {
  s: string;
  g: string;
}

export interface SpectateResult {
  kind: string;
  outcome: string;
}

export interface SpectateRecord {
  version: unknown;
  initial_sfen: string | null;
  turns: SpectateTurn[];
  result: SpectateResult | null;
  archived: boolean;
  recording: boolean;
}

export async function sha256Hex(text: string): Promise<string> {
  const data = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(digest))
    .map(b => b.toString(16).padStart(2, "0"))
    .join("");
}

// ── routing（秘匿境界の要。淀川第三歩 §1・§3.2 / 記録係二段目 §10） ──────────

export type RouteDecision = "discard" | "spectate_fanout" | "other_player_only" | "server_handled";

const SPECTATE_TYPES = new Set(["spectate_meta", "spectate_turn", "spectate_result"]);
const SERVER_HANDLED_TYPES = new Set([
  "request_reset",
  "record_invite",
  "record_accept",
  "record_decline",
  "record_testimony",
]);

/**
 * 送信者の役割とメッセージ型から、転送先を判定する（純粋）。
 * 実際の転送・副作用は room.ts が担う。
 *
 * - "discard": 観戦者の入力。型を問わず無条件破棄（観戦者は読み取り専用）。
 * - "spectate_fanout": 記録へ反映しつつ観戦者へ fan-out（相手プレイヤーへの転送は不要）。
 * - "server_handled": DO 自身が処理する（招待・二証人・リセット等）。単純な転送ではない。
 * - "other_player_only": 対局チャネル（commit/reveal/ack/hello/reconnect 等）。相手プレイヤーのみへ。
 */
export function routeDecision(isSpectator: boolean, msgType: string): RouteDecision {
  if (isSpectator) return "discard";
  if (SPECTATE_TYPES.has(msgType)) return "spectate_fanout";
  if (SERVER_HANDLED_TYPES.has(msgType)) return "server_handled";
  return "other_player_only";
}

// ── 二証人の評決（記録係二段目 §3） ─────────────────────────────────────────

export type TestimonyVerdict =
  | { kind: "matched"; id: string; witnesses: 2 }
  | { kind: "disputed"; idA: string; idB: string }
  | { kind: "rejected" };

export function isValidTestimonyText(text: string): boolean {
  return !!text && text.length <= MAX_ARCHIVE_TEXT_BYTES;
}

/** 二つの証言テキストを突き合わせる。DO は本文を解さない——ハッシュの一致だけを見る。 */
export async function evaluateTestimonies(textA: string, textB: string): Promise<TestimonyVerdict> {
  if (!isValidTestimonyText(textA) || !isValidTestimonyText(textB)) {
    return { kind: "rejected" };
  }
  const [idA, idB] = await Promise.all([sha256Hex(textA), sha256Hex(textB)]);
  if (idA === idB) return { kind: "matched", id: idA, witnesses: 2 };
  return { kind: "disputed", idA, idB };
}

// ── envelope 構築（記録係一段目 §4-1・二段目 §3。DO は本文を解さない） ───────

export interface FinalizedEnvelope {
  finalized: true;
  text: string;
  archived_at: string;
  witnesses: number;
}

export interface DisputedEnvelope {
  finalized: false;
  disputed: true;
  texts: [string, string];
  archived_at: string;
}

export interface FragmentEnvelope {
  finalized: false;
  record: {
    version: unknown;
    initial_sfen: string | null;
    turns: SpectateTurn[];
    result: SpectateResult | null;
  };
  archived_at: string;
}

const nowIso = () => new Date().toISOString();

export function buildFinalizedEnvelope(text: string, witnesses: number): FinalizedEnvelope {
  return { finalized: true, text, archived_at: nowIso(), witnesses };
}

export function buildDisputedEnvelope(texts: [string, string]): DisputedEnvelope {
  return { finalized: false, disputed: true, texts, archived_at: nowIso() };
}

export function buildFragmentEnvelope(record: SpectateRecord): FragmentEnvelope {
  return {
    finalized: false,
    record: {
      version: record.version,
      initial_sfen: record.initial_sfen,
      turns: record.turns,
      result: record.result,
    },
    archived_at: nowIso(),
  };
}

// ── 「綴じてから拭く」ゲート（記録係一段目 §4・二段目 §4） ───────────────────

/** 招かれた対局（recording）で、未綴じ・着手ありのときだけ断片として綴じてよい。 */
export function shouldArchiveFragment(recording: boolean, archived: boolean, turnsCount: number): boolean {
  return recording && !archived && turnsCount > 0;
}

/** turns 配列に新しい組手を追記してよいか（上限ガード）。 */
export function canAppendTurn(currentCount: number): boolean {
  return currentCount < MAX_TURNS;
}
