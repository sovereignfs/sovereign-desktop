import { describe, expect, it } from 'vitest';
import { normalizeInstanceUrl, parseInstanceResponse } from '../validate';

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

describe('parseInstanceResponse', () => {
  const validBody = {
    status: 'ok',
    product: 'sovereign',
    instanceName: 'My Workspace',
    platformVersion: '0.57.0',
  };

  it('parses a genuine /api/instance response', () => {
    expect(parseInstanceResponse(200, validBody)).toEqual({
      instanceName: 'My Workspace',
      platformVersion: '0.57.0',
    });
  });

  it('rejects non-200 statuses', () => {
    expect(parseInstanceResponse(403, validBody)).toBeNull();
    expect(parseInstanceResponse(404, validBody)).toBeNull();
  });

  it('rejects a body without status: ok', () => {
    expect(parseInstanceResponse(200, { ...validBody, status: 'degraded' })).toBeNull();
  });

  it('rejects a non-Sovereign server that happens to answer status: ok', () => {
    expect(parseInstanceResponse(200, { status: 'ok' })).toBeNull();
    expect(parseInstanceResponse(200, { status: 'ok', product: 'not-sovereign' })).toBeNull();
  });

  it('rejects a body missing instanceName or platformVersion', () => {
    const { instanceName: _instanceName, ...withoutName } = validBody;
    expect(parseInstanceResponse(200, withoutName)).toBeNull();
    const { platformVersion: _platformVersion, ...withoutVersion } = validBody;
    expect(parseInstanceResponse(200, withoutVersion)).toBeNull();
  });

  it('rejects malformed bodies', () => {
    expect(parseInstanceResponse(200, {})).toBeNull();
    expect(parseInstanceResponse(200, null)).toBeNull();
    expect(parseInstanceResponse(200, 'ok')).toBeNull();
  });
});
