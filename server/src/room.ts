export class GameRoom implements DurableObject {
  private state: DurableObjectState;

  constructor(state: DurableObjectState) {
    this.state = state;
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    // 診断エンドポイント: GET /room/:key/status
    if (request.method === "GET" && url.pathname.endsWith("/status")) {
      const gameStarted = (await this.state.storage.get<boolean>("gameStarted")) ?? false;
      const connections = this.state.getWebSockets().length;
      return new Response(JSON.stringify({ gameStarted, connections }, null, 2), {
        headers: { "Content-Type": "application/json" },
      });
    }

    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("WebSocket required", { status: 426 });
    }

    const existing = this.state.getWebSockets();
    const gameStarted =
      (await this.state.storage.get<boolean>("gameStarted")) ?? false;

    if (existing.length >= 2) {
      return new Response(JSON.stringify({ type: "room_full" }), {
        status: 403,
        headers: { "Content-Type": "application/json" },
      });
    }

    const { 0: client, 1: server } = new WebSocketPair();
    this.state.acceptWebSocket(server);

    if (!gameStarted) {
      // 新規ゲームフロー
      if (existing.length === 1) {
        // 2人目が入室 → 先後確定
        existing[0].send(JSON.stringify({ type: "peer_joined", your_side: "sente" }));
        server.send(JSON.stringify({ type: "room_ready", your_side: "gote" }));
        await this.state.storage.put("gameStarted", true);
      }
      // 1人目は入室待ち → 何も送らない
    } else {
      // 対局開始済み → 再接続フロー
      if (existing.length === 1) {
        // 残留プレイヤーへ通知
        existing[0].send(JSON.stringify({ type: "peer_reconnected" }));
        // 再接続プレイヤーへ通知
        server.send(JSON.stringify({ type: "you_reconnected" }));
      } else {
        // existing.length === 0: 全員切断後の最初の接続
        // stale な gameStarted をリセットして新規ゲームとして扱う
        await this.state.storage.delete("gameStarted");
      }
    }

    return new Response(null, { status: 101, webSocket: client });
  }

  webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): void {
    // request_reset: stale WS / zombie WS の強制クリーンアップ
    if (typeof message === "string") {
      try {
        const msg = JSON.parse(message) as { type?: string };
        if (msg.type === "request_reset") {
          void this.state.storage.delete("gameStarted");
          for (const other of this.state.getWebSockets()) {
            if (other !== ws) {
              try { other.close(1000, "room reset"); } catch {}
            }
          }
          ws.close(1000, "room reset");
          return;
        }
      } catch {
        // JSON でない場合は通常リレー
      }
    }
    // 通常リレー
    for (const other of this.state.getWebSockets()) {
      if (other !== ws) {
        other.send(message);
      }
    }
  }

  webSocketClose(ws: WebSocket): void {
    const others = this.state.getWebSockets().filter(s => s !== ws);

    for (const other of others) {
      try {
        other.send(JSON.stringify({ type: "peer_disconnected" }));
      } catch {
        // other socket already closed
      }
    }

    // 両者切断したら gameStarted をリセット → 同じルームキーで新規ゲームを開始できる
    if (others.length === 0) {
      void this.state.storage.delete("gameStarted");
    }
  }

  webSocketError(ws: WebSocket): void {
    this.webSocketClose(ws);
  }
}
