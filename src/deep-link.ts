/**
 * Pure helper for resolving a `sovereign://` deep link against the stored
 * instance list — no Tauri APIs, unit-tested in
 * src/__tests__/deep-link.test.ts.
 *
 * The Rust side (`src-tauri/src/lib.rs`) never resolves a deep link itself;
 * it only ever hands the raw URL to this page as `?deeplink=`, on both cold
 * launch and while already running (see that file's `navigate_to_deep_link`
 * doc comment for why). Matching against stored instances only makes sense
 * here in TypeScript, where the store already lives.
 */
import type { InstanceEntry } from './store';

/** The deep link's target instance is already stored — navigate straight to it. */
export interface KnownDeepLink {
  origin: string;
  path: string;
}

/** The deep link's target instance isn't stored yet — prompt to add it first. */
export interface UnknownDeepLink {
  unknownHost: string;
  path: string;
}

/**
 * Parse a `sovereign://<instance-host>/<path>` URL and match its host
 * against the stored instances by hostname (ignoring any port on the deep
 * link itself — the stored instance's own origin, port included, is what's
 * actually navigated to). Returns `null` for anything that isn't a
 * well-formed `sovereign://` URL with a host, rather than throwing.
 */
export function resolveDeepLink(
  rawUrl: string,
  instances: InstanceEntry[],
): KnownDeepLink | UnknownDeepLink | null {
  let url: URL;
  try {
    url = new URL(rawUrl);
  } catch {
    return null;
  }

  if (url.protocol !== 'sovereign:' || url.hostname === '') return null;

  const path = `${url.pathname}${url.search}${url.hash}` || '/';
  const match = instances.find((instance) => new URL(instance.url).hostname === url.hostname);

  return match ? { origin: match.url, path } : { unknownHost: url.hostname, path };
}
