import type { CopyDto } from "./generated/CopyDto";

/** Never "sent": macOS only tells Folyvo that the other app took the file. Same words as the CLI. */
export function handedLabel(service: string): string {
  switch (service) {
    case "mail":
      return "Attached in Mail";
    case "airdrop":
      return "AirDrop";
    case "messages":
      return "Messages";
    case "copy":
      return "Copied";
    default:
      return service.startsWith("other:") ? service.slice("other:".length) : service;
  }
}

export function copyLabel(c: CopyDto): string {
  return c.kind === "saved" ? "Saved a copy" : handedLabel(c.service ?? "");
}
