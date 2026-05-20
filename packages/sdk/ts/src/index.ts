import createClient from "openapi-fetch";
import type { paths, components } from "./generated/schema";

export { createClient };
export type { paths, components };

export type AemeathClient = ReturnType<typeof createClient<paths>>;
