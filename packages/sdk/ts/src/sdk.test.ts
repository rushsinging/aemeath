import { describe, expect, it } from "vitest";
import {
  buildBoardWebSocketUrl,
  createIdempotencyKey,
  createMessageRequest,
} from "./index";

describe("SDK helpers", () => {
  it("builds board websocket url from http base url and encodes workspace/token", () => {
    const url = buildBoardWebSocketUrl({
      baseUrl: "https://api.example.com/root/",
      workspaceId: "workspace 1",
      token: "token+/=",
    });

    expect(url).toBe(
      "wss://api.example.com/root/ws/workspaces/workspace%201/board?token=token%2B%2F%3D",
    );
  });

  it("generates unique idempotency keys with sdk prefix", () => {
    const first = createIdempotencyKey();
    const second = createIdempotencyKey();

    expect(first).toMatch(/^sdk_/);
    expect(second).toMatch(/^sdk_/);
    expect(first).not.toBe(second);
  });

  it("creates message request with generated idempotency key when omitted", () => {
    const request = createMessageRequest({ content: "hello" });

    expect(request.content).toBe("hello");
    expect(request.idempotency_key).toMatch(/^sdk_/);
  });
});
