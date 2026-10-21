import css from "./styles/app.css?raw";

test("motion is switched off for people who asked the system for less of it", () => {
  const block = /@media \(prefers-reduced-motion: reduce\)\s*\{[^}]*\{[^}]*\}[^}]*\}/s.exec(css);
  expect(block, "a prefers-reduced-motion block must exist").not.toBeNull();
  expect(block![0]).toContain("animation: none");
  expect(block![0]).toContain("transition: none");
});

test("the only keyframe animation is the share-ready moment", () => {
  const names = [...css.matchAll(/@keyframes\s+([\w-]+)/g)].map((m) => m[1]);
  expect(names).toEqual(["marked"]);
});
