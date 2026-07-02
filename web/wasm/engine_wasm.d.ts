/* tslint:disable */
/* eslint-disable */

/**
 * 対局データを版タプル付きアーカイブ書式 v1 のテキストへ変換する。
 *
 * request_json:
 * `{"initial_sfen":"...","plies":[{"s":"7g7f","g":"3c3d"},...],
 *   "rule":"0.5","protocol":2,"app":"0.8.0","sente":null,"gote":null,
 *   "result":{"kind":"mate","outcome":"gote_wins"}}`
 *
 * 成功: アーカイブ本文の文字列
 * 失敗: `"ERROR: <理由>"`
 */
export function build_archive(request_json: string): string;

/**
 * 指定局面のゲーム状態を返す（着手選択前の確定詰みチェック）。
 *
 * 返値: "ongoing" | "sente_loses" | "gote_loses" | "draw" | "error"
 */
export function game_status(sfen: string): string;

/**
 * 指定局面・陣営の合法手を USI 文字列の JSON 配列として返す。
 *
 * - sfen: 局面の SFEN 文字列
 * - side: "sente" | "gote"
 *
 * 返値: `["7g7f","P*5e",...]`（空なら `[]`）
 */
export function legal_actions(sfen: string, side: string): string;

/**
 * アーカイブ書式 v1（または旧 sfen 始まり）のテキストを解釈して対局データを返す。
 * `build_archive` の対。
 *
 * 成功: `{"ok":true,"initial_sfen":"...","plies":[{"s":"7g7f","g":"3c3d"},...],
 *        "meta":{"rule":"0.5","protocol":2,"app":"0.8.0","sente":null,"gote":null,
 *                "result":{"kind":"mate","outcome":"gote_wins"}}}`
 * 失敗: `{"ok":false,"error":"<理由>"}`（着手数超過時は `"too_many_plies"`）
 */
export function parse_archive(text: string): string;

/**
 * 両着手を解決して次局面と発生事象を返す。
 *
 * - sfen: 現局面の SFEN 文字列
 * - sente_usi: 先手の USI 着手（例: "7g7f", "P*8f", "8h3c+"）
 * - gote_usi:  後手の USI 着手
 *
 * 成功: `{"ok":true,"sfen":"<次局面>","event":"normal|clash|sente_died|gote_died|both_died"}`
 * 失敗: `{"ok":false,"error":"<理由>"}`
 */
export function resolve_ply(sfen: string, sente_usi: string, gote_usi: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly build_archive: (a: number, b: number) => [number, number];
    readonly game_status: (a: number, b: number) => [number, number];
    readonly legal_actions: (a: number, b: number, c: number, d: number) => [number, number];
    readonly parse_archive: (a: number, b: number) => [number, number];
    readonly resolve_ply: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
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
