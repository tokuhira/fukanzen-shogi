export class GameRoom implements DurableObject {
  private state: DurableObjectState;
  private readonly _key: string;

  constructor(state: DurableObjectState) {
    this.state = state;
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

    this.log(`connect existing=${existing.length} gameStarted=${gameStarted}`);

    if (existing.length >= 2) {
      this.log("rejected: room_full");
      return new Response(JSON.stringify({ type: "room_full" }), {
        status: 403,
        headers: { "Content-Type": "application/json" },
      });
    }

    const { 0: client, 1: server } = new WebSocketPair();
    this.state.acceptWebSocket(server);

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
      } else {
        await this.state.storage.delete("gameStarted");
        this.log("stale gameStarted cleared (existing=0)");
      }
    }

    return new Response(null, { status: 101, webSocket: client });
  }

  webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): void {
    if (typeof message === "string") {
      try {
        const msg = JSON.parse(message) as { type?: string };
        this.log(`recv type=${msg.type ?? "unknown"}`);
        if (msg.type === "request_reset") {
          this.log("request_reset: clearing gameStarted and closing all WSs");
          void this.state.storage.delete("gameStarted");
          for (const other of this.state.getWebSockets()) {
            if (other !== ws) {
              try { other.close(1000, "room reset"); } catch {}
            }
          }
          ws.close(1000, "room reset");
          return;
        }
      } catch {}
    }
    for (const other of this.state.getWebSockets()) {
      if (other !== ws) {
        other.send(message);
      }
    }
  }

  webSocketClose(ws: WebSocket): void {
    const others = this.state.getWebSockets().filter(s => s !== ws);
    this.log(`close remaining=${others.length}`);

    for (const other of others) {
      try {
        other.send(JSON.stringify({ type: "peer_disconnected" }));
      } catch {}
    }

    if (others.length === 0) {
      void this.state.storage.delete("gameStarted");
      this.log("all disconnected: gameStarted cleared");
    }
  }

  webSocketError(ws: WebSocket): void {
    this.log("webSocketError → webSocketClose");
    this.webSocketClose(ws);
  }
}
