import { decodeBinary, type BinaryHeader } from "../../src/protocol";

export type ConsoleSocket = {
  send: (message: unknown) => void;
  close: () => void;
};

export type SocketEvents = {
  onStatus?: (status: "connecting" | "open" | "closed") => void;
  onJson?: (message: any) => void;
  onImage?: (header: BinaryHeader, blob: Blob) => void;
};

/**
 * Console side of the hub socket. Reconnects with a capped backoff so a laptop
 * that sleeps for a while picks the session back up without a reload.
 */
export function connectConsole(token: string, events: SocketEvents): ConsoleSocket {
  let socket: WebSocket | null = null;
  let closed = false;
  let attempt = 0;
  let timer: number | null = null;
  const queue: string[] = [];

  const url = () => {
    const scheme = location.protocol === "https:" ? "wss" : "ws";
    return `${scheme}://${location.host}/v1/ws/console?token=${encodeURIComponent(token)}`;
  };

  const open = () => {
    events.onStatus?.("connecting");
    socket = new WebSocket(url());
    socket.binaryType = "arraybuffer";

    socket.onopen = () => {
      attempt = 0;
      events.onStatus?.("open");
      for (const message of queue.splice(0)) socket?.send(message);
    };

    socket.onmessage = (event) => {
      if (typeof event.data === "string") {
        try {
          events.onJson?.(JSON.parse(event.data));
        } catch {
          /* ignore malformed frames */
        }
        return;
      }
      const { header, payload } = decodeBinary(event.data as ArrayBuffer);
      const blob = new Blob([payload as BlobPart], { type: header.mime });
      events.onImage?.(header, blob);
    };

    socket.onclose = () => {
      events.onStatus?.("closed");
      socket = null;
      if (closed) return;
      const delay = Math.min(8000, 500 * 2 ** attempt++);
      timer = window.setTimeout(open, delay);
    };

    socket.onerror = () => socket?.close();
  };

  open();

  return {
    send: (message: unknown) => {
      const text = JSON.stringify(message);
      if (socket?.readyState === WebSocket.OPEN) socket.send(text);
      else queue.push(text);
    },
    close: () => {
      closed = true;
      if (timer) window.clearTimeout(timer);
      socket?.close();
    },
  };
}
