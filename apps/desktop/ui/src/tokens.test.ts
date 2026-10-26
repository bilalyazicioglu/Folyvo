import tokens from "./styles/tokens.css?raw";

const [light, dark = ""] = tokens.split("@media (prefers-color-scheme: dark)");

/** A token's colour in a scheme; the dark block overrides the light one and falls back to it. */
function colour(name: string, scheme: "light" | "dark"): [number, number, number] {
  const pattern = new RegExp(`--${name}:\\s*#([0-9a-fA-F]{6})`);
  const m = (scheme === "dark" && pattern.exec(dark)) || pattern.exec(light);
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

function contrast(a: string, b: string, scheme: "light" | "dark"): number {
  const [hi, lo] = [luminance(colour(a, scheme)), luminance(colour(b, scheme))].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const PAIRS = [
  ["ink", "bg"],
  ["ink", "surface"],
  ["muted", "bg"],
  ["muted", "surface"],
  ["accent-ink", "accent"],
  ["accent-text", "bg"],
  ["accent-text", "surface"],
  ["danger", "surface"],
  ["danger", "bg"],
];

test.each(PAIRS)("text %s on %s has at least 4.5:1 contrast", (fg, bg) => {
  expect(contrast(fg, bg, "light")).toBeGreaterThanOrEqual(4.5);
});

test("a dark scheme exists", () => {
  expect(dark).toContain("--bg:");
});

test.each(PAIRS)("in the dark scheme text %s on %s has at least 4.5:1 contrast", (fg, bg) => {
  expect(contrast(fg, bg, "dark")).toBeGreaterThanOrEqual(4.5);
});
