import net from 'node:net';

export interface CoreEntity {
  id: string;
  pluginId: string;
  entityType: string;
  projectId: string | null;
  data: unknown;
  createdAt: string;
  updatedAt: string;
}

export interface EntityClient {
  list(
    entityType: string,
    opts?: { projectId?: string; limit?: number; offset?: number }
  ): Promise<CoreEntity[]>;
  get(id: string): Promise<CoreEntity | null>;
  create(
    entityType: string,
    data: unknown,
    opts?: { projectId?: string }
  ): Promise<CoreEntity>;
  update(id: string, data: unknown): Promise<CoreEntity>;
  delete(id: string): Promise<void>;
  link(
    fromType: string,
    fromId: string,
    relation: string,
    target: { kind?: 'plugin' | 'core'; type: string; id: string }
  ): Promise<void>;
  unlink(fromId: string, toId: string, relation: string): Promise<void>;
  listRelations(id: string): Promise<unknown[]>;
}

export interface WorkItemClient {
  get(id: string): Promise<unknown>;
  list(projectId: string): Promise<unknown[]>;
}

export interface TriggerEventChannel {
  id: string;
  threadId?: string;
  name?: string;
  workspaceId?: string;
}

export interface TriggerEventUser {
  id: string;
  displayName?: string;
  bot?: boolean;
}

export interface TriggerEventPayload {
  eventId: string;
  pluginId: string;
  kind: string;
  channel: TriggerEventChannel;
  user: TriggerEventUser;
  text: string;
  mentions?: string[];
  receivedAt: string;
  raw?: unknown;
}

export interface TriggerDispatchResult {
  duplicate: boolean;
  matchedWorkflows: number;
  matchedAgents: number;
}

export interface TriggersClient {
  /** Emit a normalized inbound event to Orbit's dispatcher. */
  emit(payload: TriggerEventPayload): Promise<TriggerDispatchResult>;
}

export interface CoreApi {
  entity: EntityClient;
  workItem: WorkItemClient;
  triggers: TriggersClient;
}

/**
 * Build a CoreApi backed by the unix socket at ORBIT_CORE_API_SOCKET. Each
 * method sends one newline-delimited JSON-RPC request and awaits the response.
 */
export function createCoreApi(socketPath: string): CoreApi {
  const send = (method: string, params: Record<string, unknown>) =>
    rpcCall(socketPath, method, params);

  return {
    entity: {
      list: async (entityType, opts = {}) => {
        const result = await send('entity.list', { entityType, ...opts });
        return Array.isArray(result) ? (result as CoreEntity[]) : [];
      },
      get: async (id) => (await send('entity.get', { id })) as CoreEntity | null,
      create: async (entityType, data, opts = {}) =>
        (await send('entity.create', { entityType, data, ...opts })) as CoreEntity,
      update: async (id, data) =>
        (await send('entity.update', { id, data })) as CoreEntity,
      delete: async (id) => {
        await send('entity.delete', { id });
      },
      link: async (fromType, fromId, relation, target) => {
        await send('entity.link', {
          fromType,
          fromId,
          relation,
          toKind: target.kind ?? 'plugin',
          toType: target.type,
          toId: target.id,
        });
      },
      unlink: async (fromId, toId, relation) => {
        await send('entity.unlink', { fromId, toId, relation });
      },
      listRelations: async (id) => {
        const result = await send('entity.list_relations', { id });
        return Array.isArray(result) ? result : [];
      },
    },
    workItem: {
      get: (id) => send('work_item.get', { id }),
      list: async (projectId) => {
        const res = (await send('work_item.list', { projectId })) as {
          items?: unknown[];
        };
        return res?.items ?? [];
      },
    },
    triggers: {
      emit: async (payload) => {
        const result = (await send('trigger.emit', payload as unknown as Record<string, unknown>)) as
          | { duplicate?: boolean; matchedWorkflows?: number; matchedAgents?: number }
          | null;
        return {
          duplicate: !!result?.duplicate,
          matchedWorkflows: result?.matchedWorkflows ?? 0,
          matchedAgents: result?.matchedAgents ?? 0,
        };
      },
    },
  };
}

async function rpcCall(
  socketPath: string,
  method: string,
  params: Record<string, unknown>
): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const id = Math.floor(Math.random() * Number.MAX_SAFE_INTEGER);
    const request = { jsonrpc: '2.0', id, method, params };
    const client = net.createConnection(socketPath);
    let buffer = '';
    client.on('connect', () => {
      client.write(JSON.stringify(request) + '\n');
    });
    client.on('data', (chunk) => {
      buffer += chunk.toString('utf8');
      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';
      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const response = JSON.parse(line) as {
            id?: number;
            result?: unknown;
            error?: { message?: string };
          };
          if (response.id !== id) continue;
          client.end();
          if (response.error) {
            reject(new Error(response.error.message ?? 'core-api error'));
          } else {
            resolve(response.result);
          }
          return;
        } catch {
          // ignore parse errors on keep-alive pings
        }
      }
    });
    client.on('error', reject);
    client.on('close', () => {
      // If we get here without resolving, the server hung up on us.
      reject(new Error('core-api connection closed without response'));
    });
  });
}
