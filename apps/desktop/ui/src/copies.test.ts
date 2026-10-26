import { copyLabel, handedLabel } from "./copies";

test("handed labels match the cli", () => {
  expect(handedLabel("mail")).toBe("Attached in Mail");
  expect(handedLabel("airdrop")).toBe("AirDrop");
  expect(handedLabel("messages")).toBe("Messages");
  expect(handedLabel("copy")).toBe("Copied");
  expect(handedLabel("other:Blip")).toBe("Blip");
});

test("a saved copy and a handed one read as what happened, never as sent", () => {
  const base = { itemId: "itm_01", versionId: "ver_01", targetPath: null, at: 0 };
  expect(copyLabel({ ...base, kind: "saved", service: null, targetPath: "/out/a.docx" })).toBe("Saved a copy");
  expect(copyLabel({ ...base, kind: "handed", service: "mail" })).toBe("Attached in Mail");
});
