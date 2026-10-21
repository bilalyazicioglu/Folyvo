import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRoot } from "react-dom/client";
import { startEventBridge } from "./events";
import { Panel } from "./panel/Panel";
import { Popover } from "./popover/Popover";
import "./styles/app.css";

const params = new URLSearchParams(window.location.search);
const client = new QueryClient({ defaultOptions: { queries: { staleTime: 0, refetchOnWindowFocus: true } } });
void startEventBridge(client);

createRoot(document.getElementById("root")!).render(
  <QueryClientProvider client={client}>
    {params.get("view") === "panel" ? <Panel itemId={params.get("item") ?? ""} /> : <Popover />}
  </QueryClientProvider>,
);
