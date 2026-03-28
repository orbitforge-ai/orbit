import { invoke } from "@tauri-apps/api/core";
import { Agent, CreateAgent } from "../types";

export const agentsApi = {
  list: (): Promise<Agent[]> =>
    invoke("list_agents"),

  create: (payload: CreateAgent): Promise<Agent> =>
    invoke("create_agent", { payload }),

  delete: (id: string): Promise<void> =>
    invoke("delete_agent", { id }),
};
