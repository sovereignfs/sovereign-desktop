import { describe, expect, it } from 'vitest';
import { instanceLabel, isHealthyResponse, normalizeInstanceUrl } from '../validate';

describe('normalizeInstanceUrl', () => {
  it('defaults to https when no scheme is given', () => {
    expect(normalizeInstanceUrl('my.sovereign.example')).toBe('https://my.sovereign.example');
  });

  it('respects an explicit http scheme (LAN / local dev instances)', () => {
    expect(normalizeInstanceUrl('http://localhost:3000')).toBe('http://localhost:3000');
  });

  it('keeps a non-default port', () => {
    expect(normalizeInstanceUrl('my.sovereign.example:8443')).toBe(
      'https://my.sovereign.example:8443',
    );
  });

  it('strips path, query, and fragment down to the origin', () => {
    expect(normalizeInstanceUrl('https://my.sovereign.example/plugins/console?x=1#top')).toBe(
      'https://my.sovereign.example',
    );
  });

  it('trims surrounding whitespace', () => {
    expect(normalizeInstanceUrl('  my.sovereign.example  ')).toBe('https://my.sovereign.example');
  });

  it('rejects empty input', () => {
    expect(normalizeInstanceUrl('')).toBeNull();
    expect(normalizeInstanceUrl('   ')).toBeNull();
  });

  it('rejects non-http(s) schemes', () => {
    expect(normalizeInstanceUrl('ftp://my.sovereign.example')).toBeNull();
    expect(normalizeInstanceUrl('file:///etc/passwd')).toBeNull();
    expect(normalizeInstanceUrl('javascript://alert(1)')).toBeNull();
  });

  it('rejects URLs with embedded credentials', () => {
    expect(normalizeInstanceUrl('https://user:pass@my.sovereign.example')).toBeNull();
  });

  it('rejects garbage that cannot parse as a URL', () => {
    expect(normalizeInstanceUrl('http://')).toBeNull();
    expect(normalizeInstanceUrl('not a url')).toBeNull();
  });
});

describe('instanceLabel', () => {
  it('uses the host, including a non-default port', () => {
    expect(instanceLabel('https://my.sovereign.example')).toBe('my.sovereign.example');
    expect(instanceLabel('http://localhost:3000')).toBe('localhost:3000');
  });
});

describe('isHealthyResponse', () => {
  it('accepts the runtime liveness probe shape', () => {
    expect(isHealthyResponse(200, { status: 'ok' })).toBe(true);
  });

  it('rejects non-200 statuses', () => {
    expect(isHealthyResponse(403, { status: 'ok' })).toBe(false);
    expect(isHealthyResponse(404, { status: 'ok' })).toBe(false);
  });

  it('rejects bodies without status: ok', () => {
    expect(isHealthyResponse(200, { status: 'degraded' })).toBe(false);
    expect(isHealthyResponse(200, {})).toBe(false);
    expect(isHealthyResponse(200, null)).toBe(false);
    expect(isHealthyResponse(200, 'ok')).toBe(false);
  });
});
