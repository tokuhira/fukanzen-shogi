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

// ── 封蝋（記録係三段目 §1-3。まだ誰も呼ばない——配線は Seal-2） ─────────────────

export const SEAL_MAGIC = "fukanzen-shogi-seal";
export const SEAL_FORMAT_VERSION = 1;

/** エンベロープに乗る封蝋（淀川 §1.4：中身ではなく取り扱い層）。 */
export interface Seal {
  alg: "Ed25519";
  kid: string;
  /** 署名対象そのもの。検証側に組み直させないため literal で同梱する（概観 §2）。 */
  statement: string;
  /** statement のバイト列に対する Ed25519 署名（base64url・64 バイト）。 */
  sig: string;
}

/** 記録係の署名鍵。rawPublicKey は JWK の x 成分そのもの（§5）。 */
export interface RecorderKey {
  privateKey: CryptoKey;
  rawPublicKey: Uint8Array;
  kid: string;
}

/** loadRecorderKey が要求する env の最小形（room.ts の Env に依存しない）。 */
export interface RecorderKeyEnv {
  RECORDER_SIGNING_KEY?: string;
}

// ── envelope 構築（記録係一段目 §4-1・二段目 §3。DO は本文を解さない） ───────

export interface FinalizedEnvelope {
  finalized: true;
  text: string;
  archived_at: string;
  witnesses: number;
  seal?: Seal;
}

export interface DisputedEnvelope {
  finalized: false;
  disputed: true;
  texts: [string, string];
  archived_at: string;
  seal?: Seal;
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

// ── base64url（Workers に組み込みが無い。btoa/atob ＋ 置換で書く） ──────────

export function toBase64Url(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromBase64Url(s: string): Uint8Array {
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/");
  const pad = b64.length % 4 === 0 ? "" : "=".repeat(4 - (b64.length % 4));
  const bin = atob(b64 + pad);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// ── kid（raw 公開鍵の SHA-256 先頭 16 hex） ─────────────────────────────────

export async function kidFromRawPublicKey(raw: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", raw);
  return Array.from(new Uint8Array(digest))
    .map(b => b.toString(16).padStart(2, "0"))
    .join("")
    .slice(0, 16);
}

// ── 封蝋 statement（行ベース・engine のアーカイブ書式に倣う） ────────────────

export type SealSubject =
  | { kind: "finalized"; id: string; witnesses: number }
  | { kind: "disputed"; id: string; text_a: string; text_b: string };

/**
 * 封蝋の署名対象を組み立てる（純粋）。
 *
 * archived_at は「これから綴じる envelope が既に決めた値」を渡すこと。
 * ここで nowIso() を呼んではならない——二重生成が偽の改竄判定を生む（概観 §2.1）。
 */
export function buildSealStatement(subject: SealSubject, archivedAt: string, kid: string): string {
  const parts = [`${SEAL_MAGIC} ${SEAL_FORMAT_VERSION}`, `kind ${subject.kind}`, `id ${subject.id}`];
  if (subject.kind === "finalized") {
    parts.push(`witnesses ${subject.witnesses}`);
  } else {
    parts.push(`text_a ${subject.text_a}`);
    parts.push(`text_b ${subject.text_b}`);
  }
  parts.push(`archived_at ${archivedAt}`);
  parts.push(`kid ${kid}`);
  return parts.join("\n");
}

// ── 鍵の読み込み（モジュール memo・§5） ──────────────────────────────────────

let _keyMemo: { secret: string; promise: Promise<RecorderKey> } | null = null;

/**
 * Secret 未設定 → null（degradation。ローカル wrangler dev や Secret 投入前の正常な状態）。
 * Secret があるが壊れている（JSON でない・JWK でない）→ throw する。
 * 呼び出し側（Seal-2）が catch してログに出し、封蝋なしで綴じる。
 */
export async function loadRecorderKey(env: RecorderKeyEnv): Promise<RecorderKey | null> {
  const secret = env.RECORDER_SIGNING_KEY;
  if (!secret) return null; // 未設定は正常（封蝋を省く）
  if (_keyMemo && _keyMemo.secret === secret) return _keyMemo.promise;

  const promise = (async (): Promise<RecorderKey> => {
    const jwk = JSON.parse(secret) as JsonWebKey;
    const privateKey = await crypto.subtle.importKey("jwk", jwk, { name: "Ed25519" }, false, ["sign"]);
    // Ed25519 の秘密鍵 JWK は公開鍵成分 x を内包する（実測確認済み）ので、
    // 公開鍵用の別 Secret も exportKey も要らない。
    if (!jwk.x) throw new Error("RECORDER_SIGNING_KEY: JWK に x（公開鍵成分）が無い");
    const rawPublicKey = fromBase64Url(jwk.x);
    const kid = await kidFromRawPublicKey(rawPublicKey);
    return { privateKey, rawPublicKey, kid };
  })();

  _keyMemo = { secret, promise };
  promise.catch(() => { if (_keyMemo?.promise === promise) _keyMemo = null; });
  return promise;
}

// ── 封蝋を押す（一方向合成・概観 §2.1） ──────────────────────────────────────

/**
 * 確定局の envelope に封蝋を押す（概観 §2.1 の一方向合成）。
 *
 * archived_at と witnesses は envelope から読む——引数で受け取らない。
 * 二度生成する経路を作らないことが、偽の「改竄」判定を構造的に防ぐ。
 *
 * @param id KV キー（= 正準本文の SHA-256）。envelope に無いので引数で受ける。
 * @param key null（Secret 未設定）なら封蝋を押さず envelope をそのまま返す。
 */
export async function sealFinalized(
  envelope: FinalizedEnvelope,
  id: string,
  key: RecorderKey | null,
): Promise<FinalizedEnvelope> {
  if (!key) return envelope;
  const statement = buildSealStatement(
    { kind: "finalized", id, witnesses: envelope.witnesses },
    envelope.archived_at,
    key.kid,
  );
  return { ...envelope, seal: await signStatement(statement, key) };
}

/**
 * 不一致の envelope に封蝋を押す。
 *
 * 封じるのは「両言い分をこう受け取った」という受領の事実であって、
 * どちらが正しいかではない（審判なし・記録係二段目 §14-2）。
 *
 * @param hashes 両証言の SHA-256。evaluateTestimonies が計算済みの値を
 *               流すこと（本文を再ハッシュしない）。
 */
export async function sealDisputed(
  envelope: DisputedEnvelope,
  id: string,
  hashes: { a: string; b: string },
  key: RecorderKey | null,
): Promise<DisputedEnvelope> {
  if (!key) return envelope;
  const statement = buildSealStatement(
    { kind: "disputed", id, text_a: hashes.a, text_b: hashes.b },
    envelope.archived_at,
    key.kid,
  );
  return { ...envelope, seal: await signStatement(statement, key) };
}

async function signStatement(statement: string, key: RecorderKey): Promise<Seal> {
  const sig = await crypto.subtle.sign(
    { name: "Ed25519" },
    key.privateKey,
    new TextEncoder().encode(statement),
  );
  return { alg: "Ed25519", kid: key.kid, statement, sig: toBase64Url(new Uint8Array(sig)) };
}

// ── 検証 ─────────────────────────────────────────────────────────────────

/** 封蝋を raw 公開鍵で検証する。例外は false に潰す（検証失敗と同じ扱い）。 */
export async function verifySeal(rawPublicKey: Uint8Array, seal: Seal): Promise<boolean> {
  try {
    const pub = await crypto.subtle.importKey("raw", rawPublicKey, { name: "Ed25519" }, false, ["verify"]);
    return await crypto.subtle.verify(
      { name: "Ed25519" },
      pub,
      fromBase64Url(seal.sig),
      new TextEncoder().encode(seal.statement),
    );
  } catch {
    return false;
  }
}
