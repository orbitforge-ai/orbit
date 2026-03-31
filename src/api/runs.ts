import { invoke } from "@tauri-apps/api/core";
import { Run, RunSummary } from "../types";

export interface ListRunsParams {
  limit?: number;
  offset?: number;
  taskId?: string;
  stateFilter?: string;
}

export const runsApi = {
  list: (params: ListRunsParams = {}): Promise<RunSummary[]> =>
    invoke("list_runs", {
      limit: params.limit ?? 100,
      offset: params.offset ?? 0,
      taskId: params.taskId ?? null,
      stateFilter: params.stateFilter && params.stateFilter !== "all" ? params.stateFilter : null,
    }),

  getActive: (): Promise<RunSummary[]> =>
    invoke("get_active_runs"),

  get: (id: string): Promise<Run> =>
    invoke("get_run", { id }),

  readLog: (runId: string): Promise<string> =>
    invoke("read_run_log", { runId }),

  listSubAgentRuns: (parentRunId: string): Promise<RunSummary[]> =>
    invoke("list_sub_agent_runs", { parentRunId }),
};
