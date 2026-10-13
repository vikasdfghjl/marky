import { describe, expect, it } from 'vitest';
import type { Token } from 'markdown-it';
import { dataLineAttr, fenceLanguage } from './markdown-plugins';

describe('fenceLanguage', () => {
  it('takes the first word of the info string', () => {
    const token = { info: 'ts title="x"' } as Token;
    expect(fenceLanguage(token)).toBe('ts');
  });

  it('lowercases the language id', () => {
    const token = { info: 'Rust' } as Token;
    expect(fenceLanguage(token)).toBe('rust');
  });
});

describe('dataLineAttr', () => {
  it('emits data-line when the token has a source map', () => {
    const token = { map: [4, 6] } as Token;
    expect(dataLineAttr(token)).toBe(' data-line="4"');
  });

  it('is empty without a map', () => {
    const token = { map: null } as Token;
    expect(dataLineAttr(token)).toBe('');
  });
});
