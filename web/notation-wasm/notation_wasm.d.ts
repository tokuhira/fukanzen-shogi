/* tslint:disable */
/* eslint-disable */

/**
 * 着手の日本語棋譜表記を返す。
 *
 * - usi:        着手の USI 表記（例: "7g7f", "7g7f+", "P*5e", "resign"）
 * - side:       着手した陣営（"sente" | "gote"）
 * - legal_json: engine-wasm の legal_actions() が返す JSON 配列
 *               例: `["7g7f","6g6f","P*5e"]`
 * - sfen:       着手前の局面 SFEN
 *
 * 成功: "７六歩"、"５八金右" 等の日本語文字列
 * 失敗（不正入力）: 空文字列
 */
export function ja_notation(usi: string, side: string, legal_json: string, sfen: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly ja_notation: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number];
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
