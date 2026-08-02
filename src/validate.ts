/**
 * Pure helpers for instance URL handling — no Tauri APIs, unit-tested in
 * src/__tests__/validate.test.ts.
 */

/**
 * Normalise raw user input into an instance origin, or return null when the
 * input cannot be a valid instance URL.
 *
 * - Defaults to `https://` when no scheme is given; an explicit `http://` is
 *   respected (self-hosters on a LAN or local dev instances).
 * - Only http(s) schemes are accepted.
 * - Any path, query, or fragment is dropped — an instance is identified by its
 *   origin.
 */
export function normalizeInstanceUrl(input: string): string | null {
  const trimmed = input.trim();
  if (trimmed === '') return null;

  const withScheme = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(trimmed) ? trimmed : `https://${trimmed}`;

  let url: URL;
  try {
    url = new URL(withScheme);
  } catch {
    return null;
  }

  if (url.protocol !== 'https:' && url.protocol !== 'http:') return null;
  if (url.hostname === '') return null;
  if (url.username !== '' || url.password !== '') return null;

  return url.origin;
}

/** Parsed `GET /api/instance` response — see {@link parseInstanceResponse}. */
export interface InstanceInfo {
  instanceName: string;
  platformVersion: string;
}

/**
 * Predicate + parser for the public `GET /api/instance` endpoint every
 * Sovereign runtime exposes (RFC 0058, sovereign epic task 20.2):
 * `200` + `{ "status": "ok", "product": "sovereign", "instanceName": string,
 * "platformVersion": string }`. Checking `product === "sovereign"` (not just
 * `status === "ok"`) rules out an unrelated server that happens to answer
 * this path with an `{ "status": "ok" }`-shaped body of its own. Returns
 * `null` for anything that doesn't match — including a non-Sovereign server,
 * an unreachable host, or a malformed response.
 */
export function parseInstanceResponse(status: number, body: unknown): InstanceInfo | null {
  if (status !== 200 || typeof body !== 'object' || body === null) return null;
  const record = body as Record<string, unknown>;
  if (record.status !== 'ok' || record.product !== 'sovereign') return null;
  if (typeof record.instanceName !== 'string' || typeof record.platformVersion !== 'string') {
    return null;
  }
  return { instanceName: record.instanceName, platformVersion: record.platformVersion };
}
