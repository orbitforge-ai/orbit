import { invoke } from "@tauri-apps/api/core";
import { BusMessage, BusSubscription, CreateBusSubscription, PaginatedBusThread } from "../types";

export const busApi = {
  listMessages: (agentId?: string, limit = 50, offset = 0): Promise<BusMessage[]> =>
    invoke("list_bus_messages", { agentId, limit, offset }),

  getBusThread: (agentId: string, limit = 50, offset = 0): Promise<PaginatedBusThread> =>
    invoke("get_bus_thread", { agentId, limit, offset }),

  listSubscriptions: (agentId?: string): Promise<BusSubscription[]> =>
    invoke("list_bus_subscriptions", { agentId }),

  createSubscription: (payload: CreateBusSubscription): Promise<BusSubscription> =>
    invoke("create_bus_subscription", { payload }),

  toggleSubscription: (id: string, enabled: boolean): Promise<void> =>
    invoke("toggle_bus_subscription", { id, enabled }),

  deleteSubscription: (id: string): Promise<void> =>
    invoke("delete_bus_subscription", { id }),
};
