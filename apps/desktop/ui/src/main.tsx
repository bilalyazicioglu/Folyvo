import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRoot } from "react-dom/client";
import { startEventBridge } from "./events";

const client = new QueryClient({ defaultOptions: { queries: { staleTime: 0, refetchOnWindowFocus: true } } });
void startEventBridge(client);

createRoot(document.getElementById("root")!).render(
  <QueryClientProvider client={client}>
    <h1>Folyvo</h1>
  </QueryClientProvider>,
);
