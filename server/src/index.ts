import { GameRoom } from "./room";

export { GameRoom };

interface Env {
  ROOM: DurableObjectNamespace;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const match = url.pathname.match(/^\/room\/([^/]+)$/);

    if (!match) {
      return new Response("Not found", { status: 404 });
    }

    const roomKey = decodeURIComponent(match[1]);
    const id = env.ROOM.idFromName(roomKey);
    const stub = env.ROOM.get(id);

    return stub.fetch(request);
  },
} satisfies ExportedHandler<Env>;
