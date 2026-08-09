import { describe, expect, it } from 'vitest';
import { resolveDeepLink } from '../deep-link';
import type { InstanceEntry } from '../store';

const instances: InstanceEntry[] = [
  { url: 'https://my.sovereign.example', label: 'My Workspace', addedAt: 0 },
  { url: 'http://localhost:3000', label: 'Local dev', addedAt: 0 },
];

describe('resolveDeepLink', () => {
  it('matches a known instance by hostname and preserves its stored origin', () => {
    expect(resolveDeepLink('sovereign://my.sovereign.example/plugins/console', instances)).toEqual({
      origin: 'https://my.sovereign.example',
      path: '/plugins/console',
    });
  });

  it('preserves query and fragment in the path', () => {
    expect(resolveDeepLink('sovereign://my.sovereign.example/search?q=x#top', instances)).toEqual({
      origin: 'https://my.sovereign.example',
      path: '/search?q=x#top',
    });
  });

  it('defaults to root when the deep link has no path', () => {
    expect(resolveDeepLink('sovereign://my.sovereign.example', instances)).toEqual({
      origin: 'https://my.sovereign.example',
      path: '/',
    });
  });

  it('matches a stored instance by hostname regardless of its port', () => {
    expect(resolveDeepLink('sovereign://localhost/tasks', instances)).toEqual({
      origin: 'http://localhost:3000',
      path: '/tasks',
    });
  });

  it('reports an unknown host instead of failing', () => {
    expect(resolveDeepLink('sovereign://not-added.example/plugins/console', instances)).toEqual({
      unknownHost: 'not-added.example',
      path: '/plugins/console',
    });
  });

  it('rejects non-sovereign schemes', () => {
    expect(resolveDeepLink('https://my.sovereign.example/plugins/console', instances)).toBeNull();
  });

  it('rejects a scheme with no host', () => {
    expect(resolveDeepLink('sovereign:///plugins/console', instances)).toBeNull();
  });

  it('rejects garbage with no sovereign:// prefix', () => {
    expect(resolveDeepLink('not a url', instances)).toBeNull();
  });

  it('matches the scheme and host case-insensitively', () => {
    expect(resolveDeepLink('SOVEREIGN://My.Sovereign.Example/plugins/console', instances)).toEqual({
      origin: 'https://my.sovereign.example',
      path: '/plugins/console',
    });
  });
});
