import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

// This standalone contract records approved primitives, not parent-file inputs.
const approved = {
  '--color-page': '#ffffff',
  '--color-surface': 'var(--color-page)',
  '--color-surface-muted': '#f4f5f7',
  '--color-text': '#202124',
  '--color-text-muted': '#5e6268',
  '--color-border': '#d4d7dc',
  '--color-control-border': '#73777f',
  '--color-primary': '#1769c2',
  '--color-primary-hover': '#12559e',
  '--color-primary-active': '#104984',
  '--color-on-primary': 'var(--color-page)',
  '--color-secondary': '#f2f3f5',
  '--color-secondary-hover': '#e6e8eb',
  '--color-secondary-active': '#d9dde2',
  '--color-focus': '#005fcc',
  '--color-danger': '#9d2424',
  '--color-warning': '#76520b',
  '--color-success': '#246234',
  '--color-disabled-text': '#62666d',
  '--font-family-body':
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
  '--font-family-mono': "ui-monospace, 'SFMono-Regular', Consolas, monospace",
  '--font-size-small': '0.875rem',
  '--font-size-body': '1rem',
  '--font-size-title': '1.625rem',
  '--font-size-heading': '1.125rem',
  '--font-weight-normal': '400',
  '--font-weight-emphasis': '600',
  '--line-height-body': '1.5',
  '--line-height-heading': '1.25',
  '--space-1': '0.25rem',
  '--space-2': '0.5rem',
  '--space-3': '0.75rem',
  '--space-4': '1rem',
  '--space-5': '1.5rem',
  '--space-6': '2rem',
  '--space-7': '3rem',
  '--border-hairline': '1px',
  '--border-focus': '2px',
  '--focus-offset': '2px',
  '--radius-control': '4px',
  '--control-min-height': '2.75rem',
  '--textarea-min-height': '7rem',
  '--measure-page': '64rem',
  '--measure-form': '42rem',
  '--measure-control-min': '14rem',
  '--measure-key': '32ch',
  '--size-visually-hidden': '1px',
  '--visually-hidden-offset': '-1px'
} as const;

type Token = keyof typeof approved;

function source(): string {
  return readFileSync(new URL('../../src/theme.css', import.meta.url), 'utf8');
}

// Deliberately accepts only this small primitive-only CSS grammar.
function primitives(css: string): Map<string, string> {
  const clean = css.replace(/\/\*[\s\S]*?\*\//g, '').trim();
  const root =
    /^@layer reset, base, components, utilities;\s*@layer base\s*\{\s*:root\s*\{([^{}]*)\}\s*\}$/.exec(
      clean
    );
  if (!root)
    throw new Error(
      'Theme must contain only ordered layers and one base :root'
    );
  const entries = new Map<string, string>();
  for (const declaration of root[1]
    .split(';')
    .map((item) => item.trim())
    .filter(Boolean)) {
    const pair = /^(color-scheme|--[a-z][a-z0-9-]*):\s*(.+)$/.exec(declaration);
    if (!pair || entries.has(pair[1]))
      throw new Error(`Invalid or duplicate primitive: ${declaration}`);
    entries.set(pair[1], pair[2].trim());
  }
  return entries;
}

function color(tokens: Map<string, string>, name: Token): string {
  const value = tokens.get(name);
  const alias = /^var\((--[a-z-]+)\)$/.exec(value ?? '');
  // The only approved color aliases point to the literal white page primitive.
  const resolved = alias ? tokens.get(alias[1]) : value;
  if (!resolved || !/^#[0-9a-f]{6}$/i.test(resolved))
    throw new Error(`Invalid color: ${name}`);
  return resolved;
}

// WCAG 2.2 sRGB relative luminance and contrast, for source arithmetic only:
// https://www.w3.org/TR/WCAG22/#dfn-relative-luminance
function linear(channel: number): number {
  return channel <= 0.04045
    ? channel / 12.92
    : ((channel + 0.055) / 1.055) ** 2.4;
}

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((offset) =>
    linear(parseInt(hex.slice(offset, offset + 2), 16) / 255)
  );
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

function contrast(foreground: string, background: string): number {
  const a = luminance(foreground);
  const b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

const surfaces: Token[] = [
  '--color-page',
  '--color-surface',
  '--color-surface-muted'
];
const primary: Token[] = [
  '--color-primary',
  '--color-primary-hover',
  '--color-primary-active'
];
const secondary: Token[] = [
  '--color-secondary',
  '--color-secondary-hover',
  '--color-secondary-active'
];
const text: Token[] = [
  '--color-text',
  '--color-text-muted',
  '--color-danger',
  '--color-warning',
  '--color-success'
];

describe('approved theme primitives (source checks, not rendered accessibility)', () => {
  it('declares exactly one approved value per primitive in one root', () => {
    expect(Object.fromEntries(primitives(source()))).toEqual({
      'color-scheme': 'light',
      ...approved
    });
  });

  it('rejects duplicate declarations even when their values are identical', () => {
    expect(() =>
      primitives(
        source().replace(
          '--color-page:',
          '--color-page: #ffffff; --color-page:'
        )
      )
    ).toThrow('duplicate primitive');
  });

  it('contains only local system font stacks and no font or external style loading', () => {
    const css = source();
    const tokens = primitives(css);
    expect(tokens.get('--font-family-body')).toBe(
      approved['--font-family-body']
    );
    expect(tokens.get('--font-family-mono')).toBe(
      approved['--font-family-mono']
    );
    expect(css).not.toMatch(/@import|@font-face|url\s*\(|https?:|\/\//i);
  });

  it('preserves the page/form measures and 44px control goal at a 16px root', () => {
    const tokens = primitives(source());
    expect(tokens.get('--measure-page')).toBe('64rem');
    expect(tokens.get('--measure-form')).toBe('42rem');
    expect(tokens.get('--control-min-height')).toBe('2.75rem');
    expect(parseFloat(tokens.get('--control-min-height')!) * 16).toBe(44);
  });

  it('checks the luminance oracle at both transfer branches and known contrast anchors', () => {
    expect(linear(0.04045)).toBeCloseTo(0.0031308049535603713, 15);
    expect(linear(0.04046)).toBeCloseTo(0.003131594552688991, 15);
    expect(luminance('#ff0000')).toBeCloseTo(0.2126, 12);
    expect(luminance('#00ff00')).toBeCloseTo(0.7152, 12);
    expect(luminance('#0000ff')).toBeCloseTo(0.0722, 12);
    expect(contrast('#000000', '#ffffff')).toBe(21);
    expect(contrast('#ffffff', '#000000')).toBe(21);
    expect(contrast('#777777', '#777777')).toBe(1);
    expect(contrast('#777777', '#ffffff')).toBeLessThan(4.5);
    expect(contrast('#767676', '#ffffff')).toBeGreaterThanOrEqual(4.5);
  });

  it.each(
    text.flatMap((foreground) =>
      surfaces.map((background) => [foreground, background] as const)
    )
  )('ordinary %s text on %s reaches 4.5:1', (foreground, background) => {
    const tokens = primitives(source());
    expect(
      contrast(color(tokens, foreground), color(tokens, background))
    ).toBeGreaterThanOrEqual(4.5);
  });

  it.each(primary)('white primary text on %s reaches 4.5:1', (background) => {
    const tokens = primitives(source());
    expect(
      contrast(color(tokens, '--color-on-primary'), color(tokens, background))
    ).toBeGreaterThanOrEqual(4.5);
  });

  it.each(
    primary.flatMap((foreground) =>
      surfaces.map((background) => [foreground, background] as const)
    )
  )('%s link text on %s reaches 4.5:1', (foreground, background) => {
    const tokens = primitives(source());
    expect(
      contrast(color(tokens, foreground), color(tokens, background))
    ).toBeGreaterThanOrEqual(4.5);
  });

  it.each(secondary)(
    'secondary text and useful boundary on %s remain distinct',
    (background) => {
      const tokens = primitives(source());
      expect(
        contrast(color(tokens, '--color-text'), color(tokens, background))
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrast(
          color(tokens, '--color-control-border'),
          color(tokens, background)
        )
      ).toBeGreaterThanOrEqual(3);
      // Gray fills alone do not identify the control; later compositions must use its stronger border.
      expect(
        contrast(color(tokens, background), color(tokens, '--color-page'))
      ).toBeLessThan(3);
    }
  );

  it.each(surfaces)(
    'useful control boundaries and offset focus on %s reach 3:1',
    (background) => {
      const tokens = primitives(source());
      for (const foreground of [
        '--color-control-border',
        '--color-focus',
        ...primary
      ] as Token[]) {
        expect(
          contrast(color(tokens, foreground), color(tokens, background))
        ).toBeGreaterThanOrEqual(3);
      }
    }
  );

  it.each(surfaces)(
    'decorative hairlines on %s are not admitted as useful control boundaries',
    (background) => {
      const tokens = primitives(source());
      expect(color(tokens, '--color-border')).not.toBe(
        color(tokens, '--color-control-border')
      );
      expect(
        contrast(color(tokens, '--color-border'), color(tokens, background))
      ).toBeLessThan(3);
    }
  );

  it('measures disabled text on its secondary fill without making an active-control compliance claim', () => {
    const tokens = primitives(source());
    expect(
      contrast(
        color(tokens, '--color-disabled-text'),
        color(tokens, '--color-secondary')
      )
    ).toBeGreaterThanOrEqual(4.5);
  });
});
