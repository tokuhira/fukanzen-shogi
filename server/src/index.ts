import { GameRoom } from "./room";

export { GameRoom };

interface Env {
  ROOM: DurableObjectNamespace;
  SPECTATE_TOKENS: KVNamespace;
  ARCHIVES: KVNamespace;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    // 書庫からの直接取り出し: GET /archive/:id（部屋 DO を介さない。書庫は
    // 部屋のライフサイクルから独立。記録係一段目 §7）。
    const archiveMatch = url.pathname.match(/^\/archive\/([^/]+)$/);
    if (archiveMatch && request.method === "GET") {
      const id = decodeURIComponent(archiveMatch[1]);
      const raw = await env.ARCHIVES.get(id);
      if (!raw) {
        return new Response("Not found", { status: 404 });
      }
      return new Response(raw, { headers: { "Content-Type": "application/json" } });
    }

    // 観戦: /watch/:token → KV で roomKey に解決し、該当 DO へ spectator として委譲
    // （room key を知らせず、読み取り専用の別トークンで入れる。淀川第三歩 §4）。
    const watchMatch = url.pathname.match(/^\/watch\/([^/]+)$/);
    if (watchMatch) {
      const token = decodeURIComponent(watchMatch[1]);
      const roomKey = await env.SPECTATE_TOKENS.get(token);
      if (!roomKey) {
        return new Response("Not found", { status: 404 });
      }
      const id = env.ROOM.idFromName(roomKey);
      const stub = env.ROOM.get(id);
      const spectateUrl = new URL(request.url);
      spectateUrl.pathname = `/room/${encodeURIComponent(roomKey)}/spectate`;
      const forwarded = new Request(spectateUrl.toString(), request);
      return stub.fetch(forwarded);
    }

    const match = url.pathname.match(/^\/room\/([^/]+?)(\/status|\/archive|\/spectate)?$/);
    if (!match) {
      return new Response("Not found", { status: 404 });
    }

    const roomKey = decodeURIComponent(match[1]);
    const id = env.ROOM.idFromName(roomKey);
    const stub = env.ROOM.get(id);

    return stub.fetch(request);
  },
} satisfies ExportedHandler<Env>;
