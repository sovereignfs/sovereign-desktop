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

const SCHEME = 'sovereign://';

/**
 * Parse a `sovereign://<instance-host>/<path>` URL and match its host
 * against the stored instances by hostname (ignoring any port on the deep
 * link itself — the stored instance's own origin, port included, is what's
 * actually navigated to). Returns `null` for anything that isn't a
 * well-formed `sovereign://` URL with a host, rather than throwing.
 *
 * **Deliberately does not use `new URL(rawUrl)` to read the deep link's own
 * host/path.** `sovereign-mobile`'s port of this exact function (epic task
 * 20.14) found, via live on-device debugging, that Chromium WebView does
 * not populate `.hostname`/`.host` at all for a non-special scheme like
 * `sovereign:` — `new URL('sovereign://host/path').hostname` comes back
 * `''`, with the entire `//host/path` folded into `.pathname` instead.
 * WebKit (this app's macOS WKWebView) parses the identical string
 * correctly, and so does Node (what this file's own Vitest suite runs
 * under) — meaning a green test suite here does not prove this works on
 * WebView2, this app's Windows engine, which is Chromium-based and the
 * most likely engine to share Android's exact gap. **Applied here
 * defensively, without a confirmed reproduction on real WebView2** — this
 * machine has no Windows access (see CLAUDE.md's Windows verification
 * notes) — parsing the scheme/host/path directly from the string instead
 * of trusting the URL parser's authority recognition for anything but the
 * *stored* instance URLs below, which are ordinary `https://` URLs and
 * parse identically everywhere regardless of this gap.
 */
export function resolveDeepLink(
  rawUrl: string,
  instances: InstanceEntry[],
): KnownDeepLink | UnknownDeepLink | null {
  if (rawUrl.slice(0, SCHEME.length).toLowerCase() !== SCHEME) return null;

  const rest = rawUrl.slice(SCHEME.length);
  const slashIndex = rest.indexOf('/');
  const host = (slashIndex === -1 ? rest : rest.slice(0, slashIndex)).toLowerCase();
  if (host === '') return null;

  const path = slashIndex === -1 ? '/' : rest.slice(slashIndex);
  const match = instances.find((instance) => new URL(instance.url).hostname === host);

  return match ? { origin: match.url, path } : { unknownHost: host, path };
}
