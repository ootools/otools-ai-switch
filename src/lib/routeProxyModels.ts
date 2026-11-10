import type { FetchedRouteModel } from "./api/types";
import { getTransport } from "./transport";

/** Read the host's AI listener, including when this view runs remotely. */
export function fetchRouteProxyModels(platform: string): Promise<FetchedRouteModel[]> {
  return getTransport().call("list_route_proxy_models", { platform });
}
