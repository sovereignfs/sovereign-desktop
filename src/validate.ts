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

/** Human-readable label for a stored instance: host (plus port when non-default). */
export function instanceLabel(origin: string): string {
  try {
    return new URL(origin).host;
  } catch {
    return origin;
  }
}

/**
 * Predicate for the public `GET /api/health` liveness probe every Sovereign
 * runtime exposes: `200` + `{ "status": "ok" }`.
 */
export function isHealthyResponse(status: number, body: unknown): boolean {
  return (
    status === 200 &&
    typeof body === 'object' &&
    body !== null &&
    (body as Record<string, unknown>).status === 'ok'
  );
}
