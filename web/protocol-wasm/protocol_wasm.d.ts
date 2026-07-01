/* tslint:disable */
/* eslint-disable */

/**
 * ブラウザ手元で動く秘匿対戦プロトコルの状態機械。
 *
 * WebSocket の送受信は JS の殻が担う。このクラスは
 * 「届いたメッセージを feed() に渡すと状態が進み、次に送るべき
 * メッセージが返る」という純粋ロジックだけを保持する。
 */
export class ProtocolSession {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * peer reveal の検証後に ack メッセージを生成する。
     */
    ack_msg(): string;
    /**
     * 自分の着手を確定し commit を生成する。返り値に送るべき commit JSON を含む。
     *
     * 返り値: `{"ok":true,"message":{...},"both_committed":false}`
     * `both_committed` が true なら直ちに reveal_msg() を呼んでよい。
     */
    commit_move(sfen: string, usi: string): string;
    /**
     * 相手から届いたメッセージを処理し、状態変化を JSON で返す。
     *
     * 返り値の形式:
     * - `{"ok":true,"event":"handshake_done","peer_side":"gote"}`
     * - `{"ok":true,"event":"peer_committed","both_committed":true}`
     * - `{"ok":true,"event":"peer_revealed","both_revealed":true}`
     * - `{"ok":true,"event":"turn_complete","sente_usi":"7g7f","gote_usi":"3c3d"}`
     * - `{"ok":true,"event":"peer_reconnect_request","auth_hash":"...","board_hash":"..."}`
     * - `{"ok":true,"event":"reconnect_ack","resume_hash":"..."}`
     * - `{"ok":false,"error":"..."}`
     */
    feed(msg: string): string;
    /**
     * 接続直後に相手へ送る hello メッセージ（JSON 文字列）を返す。
     * バージョン情報・認証ハッシュ・陣営を含む。
     */
    hello_msg(): string;
    /**
     * セッションを生成する。
     * - `side`: `"sente"` または `"gote"`
     * - `secret`: 対戦相手と共有するパスワード
     */
    constructor(side: string, secret: string);
    /**
     * 初回 hello で受け取った相手の auth_hash（hex）を返す。
     * 再接続時の本人確認に使う。未取得の場合は空文字列。
     */
    peer_auth_hash(): string;
    /**
     * 再接続時に相手へ送るメッセージ（JSON 文字列）を返す。
     * - `board_hash_hex`: 現在局面の盤面ハッシュ（sfen_hash() で計算）
     */
    reconnect_msg(board_hash_hex: string): string;
    /**
     * 両者 commit 後に reveal メッセージを生成する。返り値に送るべき reveal JSON を含む。
     */
    reveal_msg(): string;
}

/**
 * SFEN 文字列から盤面ハッシュ（hex 文字列）を計算する。
 * 再接続時のハッシュ照合に使う。
 */
export function sfen_hash(sfen: string): string;

/**
 * このビルドが実装するルール・プロトコル・アプリの版タプルを JSON で返す。
 *
 * 返値: `{"rule":"0.5","protocol":2,"app":"0.8.0"}`
 */
export function version_tuple(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_protocolsession_free: (a: number, b: number) => void;
    readonly protocolsession_ack_msg: (a: number) => [number, number];
    readonly protocolsession_commit_move: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly protocolsession_feed: (a: number, b: number, c: number) => [number, number];
    readonly protocolsession_hello_msg: (a: number) => [number, number];
    readonly protocolsession_new: (a: number, b: number, c: number, d: number) => number;
    readonly protocolsession_peer_auth_hash: (a: number) => [number, number];
    readonly protocolsession_reconnect_msg: (a: number, b: number, c: number) => [number, number];
    readonly protocolsession_reveal_msg: (a: number) => [number, number];
    readonly sfen_hash: (a: number, b: number) => [number, number];
    readonly version_tuple: () => [number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
