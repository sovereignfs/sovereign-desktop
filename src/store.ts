/**
 * Instance persistence via @tauri-apps/plugin-store — a JSON file in the app
 * data directory, surviving app restarts and updates.
 */
import { load, type Store } from '@tauri-apps/plugin-store';

export interface InstanceEntry {
  /** Instance origin, e.g. `https://my.sovereign.example`. */
  url: string;
  /** Display label — the instance host. */
  label: string;
  /** Unix epoch ms when the instance was added. */
  addedAt: number;
}

const STORE_FILE = 'instances.json';
const KEY_INSTANCES = 'instances';
const KEY_ACTIVE_URL = 'activeUrl';

let storePromise: Promise<Store> | null = null;

function getStore(): Promise<Store> {
  storePromise ??= load(STORE_FILE);
  return storePromise;
}

export async function listInstances(): Promise<InstanceEntry[]> {
  const store = await getStore();
  return (await store.get<InstanceEntry[]>(KEY_INSTANCES)) ?? [];
}

export async function getActiveUrl(): Promise<string | null> {
  const store = await getStore();
  return (await store.get<string>(KEY_ACTIVE_URL)) ?? null;
}

export async function setActiveUrl(url: string | null): Promise<void> {
  const store = await getStore();
  if (url === null) {
    await store.delete(KEY_ACTIVE_URL);
  } else {
    await store.set(KEY_ACTIVE_URL, url);
  }
  await store.save();
}

/** Add an instance (no-op when the URL is already stored) and make it active. */
export async function addInstance(entry: InstanceEntry): Promise<void> {
  const store = await getStore();
  const instances = await listInstances();
  if (!instances.some((i) => i.url === entry.url)) {
    instances.push(entry);
    await store.set(KEY_INSTANCES, instances);
  }
  await store.set(KEY_ACTIVE_URL, entry.url);
  await store.save();
}

/** Remove an instance; clears the active URL when it pointed at the removed entry. */
export async function removeInstance(url: string): Promise<void> {
  const store = await getStore();
  const instances = (await listInstances()).filter((i) => i.url !== url);
  await store.set(KEY_INSTANCES, instances);
  if ((await getActiveUrl()) === url) {
    await store.delete(KEY_ACTIVE_URL);
  }
  await store.save();
}
