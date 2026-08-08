/**
 * Boot: load the active instance straight away when one is stored (the local
 * page acts as a brief splash), otherwise render onboarding. The native
 * "Instances → Switch Instance…" menu item navigates back here with
 * `?manage=1`, which forces the manager view even when an instance is active.
 *
 * A `sovereign://` deep link arrives here too, as `?deeplink=<raw-url>` — the
 * Rust side (`src-tauri/src/lib.rs`) never resolves it itself, only ever
 * lands on this page with the raw URL attached, on both cold launch and
 * while already running. Checked before the stored-active-instance
 * shortcut above so a deep link always wins over just reopening whatever
 * was last active.
 */
import { getActiveUrl, listInstances, setActiveUrl } from './store';
import { renderOnboarding } from './onboarding';
import { resolveDeepLink } from './deep-link';

async function handleDeepLink(rawUrl: string, root: HTMLElement): Promise<boolean> {
  const resolved = resolveDeepLink(rawUrl, await listInstances());
  if (resolved === null) return false;

  if ('origin' in resolved) {
    await setActiveUrl(resolved.origin);
    window.location.replace(resolved.origin + resolved.path);
  } else {
    await renderOnboarding(root, {
      prefillUrl: `https://${resolved.unknownHost}`,
      pendingPath: resolved.path,
    });
  }
  return true;
}

async function boot(): Promise<void> {
  const root = document.getElementById('app');
  if (!root) return;

  const params = new URLSearchParams(window.location.search);
  const deepLink = params.get('deeplink');
  if (deepLink !== null && (await handleDeepLink(deepLink, root))) return;

  const manage = params.has('manage');
  const activeUrl = await getActiveUrl();

  if (activeUrl !== null && !manage) {
    window.location.replace(activeUrl);
    return;
  }

  await renderOnboarding(root);
}

void boot();
