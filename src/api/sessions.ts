import { invoke } from "@tauri-apps/api/core";
import { CreateSession, Session, UpdateSession } from "../types";

export const sessionsApi = {
  list: (): Promise<Session[]> =>
    invoke("list_sessions"),

  get: (id: string): Promise<Session> =>
    invoke("get_session", { id }),

  create: (payload: CreateSession): Promise<Session> =>
    invoke("create_session", { payload }),

  update: (id: string, payload: UpdateSession): Promise<Session> =>
    invoke("update_session", { id, payload }),

  delete: (id: string): Promise<void> =>
    invoke("delete_session", { id }),
};
