import {
  routeDecision,
  evaluateTestimonies,
  isValidTestimonyText,
  buildFinalizedEnvelope,
  buildDisputedEnvelope,
  buildFragmentEnvelope,
  shouldArchiveFragment,
  canAppendTurn,
  sha256Hex,
  type SpectateTurn,
  type SpectateResult,
  type SpectateRecord,
} from "./logic";

interface Env {
  SPECTATE_TOKENS: KVNamespace;
  ARCHIVES: KVNamespace;
}

interface Testimony {
  text: string;
}

export class GameRoom implements DurableObject {
  private state: DurableObjectState;
  private env: Env;
  private readonly _key: string;

  // 二証人の証言収集は単一終局分の一時状態でよい（記録係二段目 §10）。
  // ws をキーに、招かれた対局の終局時に届いた証言を溜め、綴じたらクリアする。
  private _testimonies: Map<WebSocket, Testimony> = new Map();

  constructor(state: DurableObjectState, env: Env) {
    this.state = state;
    this.env = env;
    this._key = state.id.name ?? '?';
  }

  private log(...args: unknown[]): void {
    console.log(`[room:${this._key}]`, ...args);
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    // 診断エンドポイント: GET /room/:key/status
    if (request.method === "GET" && url.pathname.endsWith("/status")) {
      const gameStarted = (await this.state.storage.get<boolean>("gameStarted")) ?? false;
      const players = this.state.getWebSockets("player").length;
      const spectators = this.state.getWebSockets("spectator").length;
      return new Response(JSON.stringify({ gameStarted, players, spectators }, null, 2), {
        headers: { "Content-Type": "application/json" },
      });
    }

    // サーバ側アーカイブの取り出し: GET /room/:key/archive（淀川第三歩 §6）。
    // 部屋のいまの一局（ライブ・診断用）。恒久の記録は /archive/:id（記録係一段目 §7）。
    if (request.method === "GET" && url.pathname.endsWith("/archive")) {
      const record = await this._loadRecord();
      return new Response(JSON.stringify(record, null, 2), {
        headers: { "Content-Type": "application/json" },
      });
    }

    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("WebSocket required", { status: 426 });
    }

    // 観戦者接続（/watch/:token 経由。index.ts が /room/:key/spectate へ書き換えて委譲）。
    // 枠制限なし・読み取り専用（淀川第三歩 §3.1・§4）。
    if (url.pathname.endsWith("/spectate")) {
      const { 0: client, 1: server } = new WebSocketPair();
      this.state.acceptWebSocket(server, ["spectator"]);
      const record = await this._loadRecord();
      server.send(JSON.stringify({ type: "spectate_init", ...record }));
      this.log("spectator connected");
      return new Response(null, { status: 101, webSocket: client });
    }

    // 2人枠は player のみで計数（観戦者は枠外。淀川第三歩 §3.1）。
    const existing = this.state.getWebSockets("player");
    const gameStarted =
      (await this.state.storage.get<boolean>("gameStarted")) ?? false;

    this.log(`connect existing=${existing.length} gameStarted=${gameStarted}`);

    if (existing.length >= 2) {
      this.log("rejected: room_full");
      return new Response(JSON.stringify({ type: "room_full" }), {
        status: 403,
        headers: { "Content-Type": "application/json" },
      });
    }

    const { 0: client, 1: server } = new WebSocketPair();
    this.state.acceptWebSocket(server, ["player"]);

    if (!gameStarted) {
      if (existing.length === 1) {
        existing[0].send(JSON.stringify({ type: "peer_joined", your_side: "sente" }));
        server.send(JSON.stringify({ type: "room_ready", your_side: "gote" }));
        await this.state.storage.put("gameStarted", true);
        this.log("game started: sente + gote assigned");
      } else {
        this.log("waiting for 2nd player");
      }
    } else {
      if (existing.length === 1) {
        existing[0].send(JSON.stringify({ type: "peer_reconnected" }));
        server.send(JSON.stringify({ type: "you_reconnected" }));
        this.log("reconnect flow: you_reconnected sent");
        this._broadcastSpectateStatus("resumed");
      } else {
        await this.state.storage.delete("gameStarted");
        this.log("stale gameStarted cleared (existing=0)");
      }
    }

    return new Response(null, { status: 101, webSocket: client });
  }

  // 送信者の役割とメッセージ型で振り分ける（淀川第三歩 §1・§3.2 の秘匿境界の急所）。
  // 判定そのものは logic.ts の routeDecision（純粋関数・テスト済み）に委ねる。
  // - "discard": 観戦者の入力。無条件破棄。
  // - "spectate_fanout": 記録へ反映しつつ観戦者へ fan-out。
  // - "server_handled": 招待・二証人・リセット等、DO 自身が個別に処理する。
  // - "other_player_only": 対局チャネル（commit/reveal/ack/hello/reconnect 等）。
  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    if (typeof message !== "string") return;

    let msg: { type?: string; [k: string]: unknown };
    try {
      msg = JSON.parse(message);
    } catch {
      return;
    }
    this.log(`recv type=${msg.type ?? "unknown"}`);

    const isSpectator = this.state.getWebSockets("spectator").includes(ws);
    const decision = routeDecision(isSpectator, msg.type ?? "");

    // 観戦者は読み取り専用。型を見るより先に、ここで無条件に破棄する（淀川第三歩
    // §1-B・§10.1）。以前は request_reset の分岐がこのチェックより先にあったため、
    // 観戦者が request_reset を送ると getWebSockets("player") 全員が
    // 「other !== ws」を満たしてしまい（観戦ソケットは player リストに元々
    // 含まれないため）、対局を強制リセットできてしまっていた。
    if (decision === "discard") return;

    if (decision === "spectate_fanout") {
      await this._applyToRecord(msg);
      if (msg.type === "spectate_meta") {
        await this._issueSpectateToken();
      }
      for (const spec of this.state.getWebSockets("spectator")) {
        try { spec.send(message); } catch {}
      }
      return;
    }

    if (decision === "server_handled") {
      if (msg.type === "request_reset") {
        this.log("request_reset: clearing gameStarted and closing other players");
        void this.state.storage.delete("gameStarted");
        for (const other of this.state.getWebSockets("player")) {
          if (other !== ws) {
            try { other.close(1000, "room reset"); } catch {}
          }
        }
        ws.close(1000, "room reset");
        return;
      }

      // 記録係の招待（記録係二段目 §2・§10）。対局チャネル——観戦者へは中継しない。
      if (msg.type === "record_invite") {
        for (const other of this.state.getWebSockets("player")) {
          if (other !== ws) { try { other.send(message); } catch {} }
        }
        return;
      }
      if (msg.type === "record_accept") {
        await this.state.storage.put("recording", true);
        const payload = JSON.stringify({ type: "record_confirmed" });
        for (const p of this.state.getWebSockets("player")) { try { p.send(payload); } catch {} }
        for (const s of this.state.getWebSockets("spectator")) { try { s.send(payload); } catch {} }
        return;
      }
      if (msg.type === "record_decline") {
        const payload = JSON.stringify({ type: "record_declined" });
        for (const other of this.state.getWebSockets("player")) {
          if (other !== ws) { try { other.send(payload); } catch {} }
        }
        return;
      }
      if (msg.type === "record_testimony") {
        await this._handleTestimony(ws, msg);
        return;
      }
      return;
    }

    // 対局チャネル（commit/reveal/ack/hello/reconnect など）は相手プレイヤーのみへ。
    for (const other of this.state.getWebSockets("player")) {
      if (other !== ws) {
        other.send(message);
      }
    }
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    const wasSpectator = this.state.getWebSockets("spectator").includes(ws);
    if (wasSpectator) {
      // 観戦者自身の切断は記録に影響しない（淀川第三歩 §3.4）。
      return;
    }

    const otherPlayers = this.state.getWebSockets("player").filter(s => s !== ws);
    this.log(`close remaining players=${otherPlayers.length}`);

    for (const other of otherPlayers) {
      try {
        other.send(JSON.stringify({ type: "peer_disconnected" }));
      } catch {}
    }
    this._broadcastSpectateStatus("player_disconnected");

    if (otherPlayers.length === 0) {
      // 全員離脱（記録係一段目 §4-3・記録係二段目 §4）。gameStarted を消す前に、
      // 招かれた対局（recording=true）なら綴じる——「綴じてから拭く」不変条件。
      // 片方だけ証言済み（相手が終局前に離脱等）なら、その一証言を witnesses:1
      // で確定綴じする（記録係二段目 §3）。それ以外は従来どおり断片綴じへ。
      const recording = (await this.state.storage.get<boolean>("recording")) ?? false;
      const archived = (await this.state.storage.get<boolean>("archived")) ?? false;
      const solo = [...this._testimonies.values()][0];
      if (recording && !archived && this._testimonies.size === 1 && solo && isValidTestimonyText(solo.text)) {
        try {
          const id = await this._archiveFinalized(solo.text, 1);
          await this.state.storage.put("archived", true);
          this._broadcastArchived(id);
        } catch (err) {
          this.log(`single-witness archive failed: ${String(err)}`);
        }
      } else {
        await this._archiveCurrentIfNeeded();
      }
      this._testimonies.clear();
      void this.state.storage.delete("gameStarted");
      this.log("all players disconnected: gameStarted cleared");
    }
  }

  webSocketError(ws: WebSocket): void {
    this.log("webSocketError → webSocketClose");
    void this.webSocketClose(ws);
  }

  // ── 公開組手の記録（淀川第三歩 §3.3・§6） ────────────────────────────────────

  private async _loadRecord(): Promise<SpectateRecord> {
    const version = (await this.state.storage.get<unknown>("version")) ?? null;
    const initial_sfen = (await this.state.storage.get<string>("initial_sfen")) ?? null;
    const turns = (await this.state.storage.get<SpectateTurn[]>("turns")) ?? [];
    const result = (await this.state.storage.get<SpectateResult>("result")) ?? null;
    const archived = (await this.state.storage.get<boolean>("archived")) ?? false;
    const recording = (await this.state.storage.get<boolean>("recording")) ?? false;
    return { version, initial_sfen, turns, result, archived, recording };
  }

  private async _applyToRecord(msg: { type?: string; [k: string]: unknown }): Promise<void> {
    if (msg.type === "spectate_meta") {
      // 新しい対局の開始（再戦を含む）。記録係一段目 §4-2: 拭く（初期化する）前に、
      // 前局が未綴じのまま残っていれば（招かれていた対局のみ）断片として書庫へ綴じる。
      await this._archiveCurrentIfNeeded();

      await this.state.storage.put("version", msg.version ?? null);
      await this.state.storage.put("initial_sfen", (msg.initial_sfen as string) ?? null);
      await this.state.storage.put("turns", []);
      await this.state.storage.put("result", null);
      await this.state.storage.put("archived", false);
      // 記録係二段目 §2: 招待は対局ごと。新局は毎回未招待から始まる。
      await this.state.storage.put("recording", false);
      this._testimonies.clear();
    } else if (msg.type === "spectate_turn") {
      const turns = (await this.state.storage.get<SpectateTurn[]>("turns")) ?? [];
      if (canAppendTurn(turns.length)) {
        turns.push({ s: String(msg.s ?? ""), g: String(msg.g ?? "") });
        await this.state.storage.put("turns", turns);
      }
    } else if (msg.type === "spectate_result") {
      // ライブの終局表示のみを担う（観戦者向け）。記録係二段目 §10: 綴じ
      // （_archiveFinalized）はここから除去し、record_testimony の二証人経路へ
      // 移した。招かれていない対局はここで終局してもそのまま揮発する。
      const result = { kind: String(msg.kind ?? ""), outcome: String(msg.outcome ?? "") };
      await this.state.storage.put("result", result);
    }
  }

  // ── 二証人の交差確認（記録係二段目 §3・§10） ─────────────────────────────────

  private async _handleTestimony(ws: WebSocket, msg: { [k: string]: unknown }): Promise<void> {
    const recording = (await this.state.storage.get<boolean>("recording")) ?? false;
    if (!recording) return; // 未招待の対局に証言は要らない

    // kind/outcome は正準本文（text）の result 行に既に埋め込まれており、
    // 一致判定・綴じのどちらでも参照しないため保持しない（受信メッセージには
    // 指示書どおり乗るが、DO は text しか解さない）。
    this._testimonies.set(ws, {
      text: typeof msg.text === "string" ? msg.text : "",
    });

    // getWebSockets("player") との突き合わせはしない。実際のクライアントは
    // 証言を送った直後に自分の ws を閉じる（endOnlineGame → disconnectOnline）ため、
    // 二人目の証言が届く頃には一人目の ws は既に「player」から外れている。
    // Map は ws（=接続）をキーにしており、同じ接続からの再送は上書きになるので、
    // size===2 は「二つの異なる接続が証言した」で足りる（記録係二段目 §10）。
    if (this._testimonies.size < 2) return; // もう一方の証言を待つ

    const [a, b] = [...this._testimonies.values()];
    this._testimonies.clear();

    const verdict = await evaluateTestimonies(a.text, b.text);

    if (verdict.kind === "rejected") {
      // 綴じない（空・上限超過）。招待済みの放棄と同じく、後で断片綴じに
      // フォールバックしうる（_archiveCurrentIfNeeded、archived はまだ false）。
      this.log("testimony rejected: empty or oversized text");
      return;
    }

    try {
      if (verdict.kind === "matched") {
        // 一致: 二人の独立した目が合致した、確かな記録。
        await this._archiveFinalized(a.text, verdict.witnesses);
        await this.state.storage.put("archived", true);
        this._broadcastArchived(verdict.id);
      } else {
        // 不一致: 裁定しない（審判なし＝版図）。両証言を保存し surface する。
        const id = await this._archiveDisputed([a.text, b.text]);
        await this.state.storage.put("archived", true);
        this._broadcastRecordDisagreement(verdict.idA, verdict.idB, id);
      }
    } catch (err) {
      this.log(`testimony archive failed: ${String(err)}`);
    }
  }

  // ── 書庫（記録係一段目 §2・§3・§4） ──────────────────────────────────────────

  // 招かれた対局（recording=true）で、未綴じ（archived=false）かつ着手のある
  // 記録を、断片として書庫へ綴じる。再戦開始（§4-2）・全員離脱（§4-3）の両方から
  // 呼ぶ共通経路。記録係二段目 §4: 未招待は綴じずに拭いてよい（意図的な揮発）。
  private async _archiveCurrentIfNeeded(): Promise<void> {
    const recording = (await this.state.storage.get<boolean>("recording")) ?? false;
    const archived = (await this.state.storage.get<boolean>("archived")) ?? false;
    const turns = (await this.state.storage.get<SpectateTurn[]>("turns")) ?? [];
    if (!shouldArchiveFragment(recording, archived, turns.length)) return;

    const record = await this._loadRecord();
    try {
      await this._archiveFragment(record);
      await this.state.storage.put("archived", true);
    } catch (err) {
      this.log(`fragment archive failed: ${String(err)}`);
    }
  }

  // 確定局: 正準アーカイブ本文の内容ハッシュ（SHA-256）を id として書庫へ綴じる。
  // DO は本文を解さない（不透明ブロブとして content-address するだけ）。
  // witnesses: 二証人が一致すれば 2、相手が終局前に離脱し片方のみなら 1
  // （記録係二段目 §3）。
  private async _archiveFinalized(text: string, witnesses: number): Promise<string> {
    const id = await sha256Hex(text);
    const envelope = buildFinalizedEnvelope(text, witnesses);
    await this.env.ARCHIVES.put(id, JSON.stringify(envelope));
    this.log(`archived finalized id=${id} witnesses=${witnesses}`);
    return id;
  }

  // 二証人が食い違った対局: 裁定せず、両証言を暫定 ID（ランダム UUID）で
  // 保存する。証拠は失わない（記録係二段目 §3）。
  private async _archiveDisputed(texts: [string, string]): Promise<string> {
    const id = crypto.randomUUID();
    const envelope = buildDisputedEnvelope(texts);
    await this.env.ARCHIVES.put(id, JSON.stringify(envelope));
    this.log(`archived disputed id=${id}`);
    return id;
  }

  // 放棄断片: 確定していない対局を暫定 ID（ランダム UUID）で書庫へ綴じる。
  private async _archiveFragment(record: SpectateRecord): Promise<string> {
    const id = crypto.randomUUID();
    const envelope = buildFragmentEnvelope(record);
    await this.env.ARCHIVES.put(id, JSON.stringify(envelope));
    this.log(`archived fragment id=${id}`);
    return id;
  }

  private _broadcastArchived(id: string): void {
    const payload = JSON.stringify({ type: "archived", id });
    for (const player of this.state.getWebSockets("player")) {
      try { player.send(payload); } catch {}
    }
    for (const spec of this.state.getWebSockets("spectator")) {
      try { spec.send(payload); } catch {}
    }
  }

  // 二証人の食い違いを、裁定せず両者＋観戦者へ透明に示す（記録係二段目 §3・§10）。
  // id_a/id_b は各人が自分の証言そのものを検証できる自己完結ハッシュ、id は
  // 両証言を保存した disputed envelope の取り出し key。
  private _broadcastRecordDisagreement(idA: string, idB: string, id: string): void {
    const payload = JSON.stringify({ type: "record_disagreement", id_a: idA, id_b: idB, id });
    for (const player of this.state.getWebSockets("player")) {
      try { player.send(payload); } catch {}
    }
    for (const spec of this.state.getWebSockets("spectator")) {
      try { spec.send(payload); } catch {}
    }
  }

  // ── 観戦トークン（淀川第三歩 §4） ────────────────────────────────────────────

  private async _issueSpectateToken(): Promise<void> {
    const previous = await this.state.storage.get<string>("spectateToken");
    if (previous) {
      // 古いリンクを無効化（新しい対局が始まったため）。
      try { await this.env.SPECTATE_TOKENS.delete(previous); } catch {}
    }

    const token = crypto.randomUUID();
    await this.env.SPECTATE_TOKENS.put(token, this._key, { expirationTtl: 60 * 60 * 24 * 30 });
    await this.state.storage.put("spectateToken", token);

    const payload = JSON.stringify({ type: "spectate_token", token });
    for (const player of this.state.getWebSockets("player")) {
      try { player.send(payload); } catch {}
    }
    this.log("spectate token issued");
  }

  private _broadcastSpectateStatus(state: "player_disconnected" | "resumed"): void {
    const payload = JSON.stringify({ type: "spectate_status", state });
    for (const spec of this.state.getWebSockets("spectator")) {
      try { spec.send(payload); } catch {}
    }
  }
}
