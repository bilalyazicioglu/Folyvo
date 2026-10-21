import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRoot } from "react-dom/client";
import { startEventBridge } from "./events";
import { Panel } from "./panel/Panel";
import { Popover } from "./popover/Popover";
import "./styles/app.css";

async function start() {
  const params = new URLSearchParams(window.location.search);
  if (params.get("demo") === "1") {
    // Fixture data for design review and screenshots; never loaded in the real app.
    (await import("./demo")).installDemo();
  }
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: 0, refetchOnWindowFocus: true } } });
  void startEventBridge(client);
  createRoot(document.getElementById("root")!).render(
    <QueryClientProvider client={client}>
      {params.get("view") === "panel" ? <Panel itemId={params.get("item") ?? ""} /> : <Popover />}
    </QueryClientProvider>,
  );
}

void start();
