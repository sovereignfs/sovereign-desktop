/**
 * Boot: load the active instance straight away when one is stored (the local
 * page acts as a brief splash), otherwise render onboarding. The native
 * "Instances → Switch Instance…" menu item navigates back here with
 * `?manage=1`, which forces the manager view even when an instance is active.
 */
import { getActiveUrl } from './store';
import { renderOnboarding } from './onboarding';

async function boot(): Promise<void> {
  const root = document.getElementById('app');
  if (!root) return;

  const manage = new URLSearchParams(window.location.search).has('manage');
  const activeUrl = await getActiveUrl();

  if (activeUrl !== null && !manage) {
    window.location.replace(activeUrl);
    return;
  }

  await renderOnboarding(root);
}

void boot();
