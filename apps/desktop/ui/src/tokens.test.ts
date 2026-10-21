import tokens from "./styles/tokens.css?raw";

function colour(name: string): [number, number, number] {
  const m = new RegExp(`--${name}:\\s*#([0-9a-fA-F]{6})`).exec(tokens);
  if (!m) throw new Error(`token --${name} not found`);
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function luminance([r, g, b]: [number, number, number]): number {
  const f = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(colour(a)), luminance(colour(b))].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

test.each([
  ["ink", "bg"],
  ["ink", "surface"],
  ["muted", "bg"],
  ["muted", "surface"],
  ["ready-ink", "ready"],
  ["amber-ink", "amber"],
  ["layer-ink", "layer-a"],
  ["layer-ink", "layer-b"],
  ["danger", "surface"],
  ["danger", "bg"],
])("text %s on %s has at least 4.5:1 contrast", (fg, bg) => {
  expect(contrast(fg, bg)).toBeGreaterThanOrEqual(4.5);
});
