/**
 * Onboarding / instance-manager view: add an instance (validated against the
 * public `GET /api/instance` endpoint), list stored instances, switch, and
 * remove. Rendered on first launch and whenever the native
 * "Instances → Switch Instance…" menu navigates back to this page.
 */
import { fetch as tauriFetch } from '@tauri-apps/plugin-http';
import { addInstance, listInstances, removeInstance, setActiveUrl } from './store';
import { normalizeInstanceUrl, parseInstanceResponse } from './validate';
import type { InstanceInfo } from './validate';

const VALIDATE_TIMEOUT_MS = 5000;

/**
 * Check that `origin` serves a genuine Sovereign instance and read its
 * display name. Uses the Tauri HTTP plugin — the request is made from the
 * Rust side, so the instance does not need CORS headers for the shell's
 * local origin.
 */
async function checkInstance(origin: string): Promise<InstanceInfo | null> {
  try {
    const res = await tauriFetch(`${origin}/api/instance`, {
      method: 'GET',
      signal: AbortSignal.timeout(VALIDATE_TIMEOUT_MS),
    });
    return parseInstanceResponse(res.status, await res.json());
  } catch {
    return null;
  }
}

function loadInstance(url: string): void {
  window.location.replace(url);
}

export async function renderOnboarding(root: HTMLElement): Promise<void> {
  const instances = await listInstances();
  const firstLaunch = instances.length === 0;

  root.innerHTML = `
    <div class="onboarding">
      <h1>Sovereign</h1>
      <p class="subtitle">${
        firstLaunch
          ? 'Connect to your self-hosted Sovereign instance.'
          : 'Choose an instance, or add another one.'
      }</p>
      <ul class="instance-list" aria-label="Your instances"></ul>
      <form class="add-form" novalidate>
        <label for="instance-url">Instance URL</label>
        <input
          id="instance-url"
          name="url"
          type="url"
          placeholder="my.sovereign.example"
          autocomplete="url"
          autocapitalize="off"
          autocorrect="off"
          spellcheck="false"
          required
        />
        <p class="form-error" role="alert" aria-live="polite"></p>
        <button type="submit">${firstLaunch ? 'Connect' : 'Add instance'}</button>
      </form>
    </div>
  `;
  root.removeAttribute('aria-busy');

  const list = root.querySelector<HTMLUListElement>('.instance-list');
  const form = root.querySelector<HTMLFormElement>('.add-form');
  const input = root.querySelector<HTMLInputElement>('#instance-url');
  const error = root.querySelector<HTMLParagraphElement>('.form-error');
  const submit = root.querySelector<HTMLButtonElement>('button[type="submit"]');
  if (!list || !form || !input || !error || !submit) return;

  for (const instance of instances) {
    const item = document.createElement('li');

    const open = document.createElement('button');
    open.type = 'button';
    open.className = 'instance-open';
    const label = document.createElement('span');
    label.className = 'label';
    label.textContent = instance.label;
    const url = document.createElement('span');
    url.className = 'url';
    url.textContent = instance.url;
    open.append(label, url);
    open.addEventListener('click', () => {
      void setActiveUrl(instance.url).then(() => loadInstance(instance.url));
    });

    const remove = document.createElement('button');
    remove.type = 'button';
    remove.className = 'instance-remove';
    remove.textContent = '✕';
    remove.setAttribute('aria-label', `Remove ${instance.label}`);
    remove.addEventListener('click', () => {
      void removeInstance(instance.url).then(() => renderOnboarding(root));
    });

    item.append(open, remove);
    list.append(item);
  }
  if (instances.length === 0) list.remove();

  form.addEventListener('submit', (event) => {
    event.preventDefault();
    void (async () => {
      error.textContent = '';

      const origin = normalizeInstanceUrl(input.value);
      if (origin === null) {
        error.textContent = 'Enter a valid URL, e.g. my.sovereign.example';
        return;
      }

      submit.disabled = true;
      submit.textContent = 'Connecting…';
      const info = await checkInstance(origin);
      submit.disabled = false;
      submit.textContent = firstLaunch ? 'Connect' : 'Add instance';

      if (info === null) {
        error.textContent = `Could not reach a Sovereign instance at ${origin}. Check the URL and try again.`;
        return;
      }

      await addInstance({ url: origin, label: info.instanceName, addedAt: Date.now() });
      loadInstance(origin);
    })();
  });

  input.focus();
}
