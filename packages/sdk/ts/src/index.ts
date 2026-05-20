import createClient from "openapi-fetch";
import type { paths, components } from "./generated/schema";

export { createClient };
export type { paths, components };

export type AemeathClient = ReturnType<typeof createClient<paths>>;
export type AddMessageRequest = components["schemas"]["AddMessageRequest"];

export interface BoardWebSocketUrlOptions {
  baseUrl: string;
  workspaceId: string;
  token?: string;
}

export function buildBoardWebSocketUrl(options: BoardWebSocketUrlOptions): string {
  const url = new URL(options.baseUrl);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  const basePath = url.pathname.replace(/\/$/, "");
  url.pathname = `${basePath}/ws/workspaces/${encodeURIComponent(options.workspaceId)}/board`;
  url.search = "";

  if (options.token) {
    url.searchParams.set("token", options.token);
  }

  return url.toString();
}

export function createIdempotencyKey(): string {
  return `sdk_${crypto.randomUUID()}`;
}

export function createMessageRequest(
  request: Omit<AddMessageRequest, "idempotency_key"> & Partial<Pick<AddMessageRequest, "idempotency_key">>,
): AddMessageRequest {
  return {
    ...request,
    idempotency_key: request.idempotency_key ?? createIdempotencyKey(),
  };
}
