/**
 * useSyncEvents — listens for `sync:remote_change` events emitted by the
 * Rust Realtime sync module and invalidates the appropriate React Query caches.
 *
 * Call this hook once inside AppContent (or any component that lives for the
 * lifetime of a logged-in session).  It is a no-op when the user is offline.
 */

import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useQueryClient } from '@tanstack/react-query';

interface RemoteChangePayload {
  table: string;
  scope_type?: string;
  scope_id?: string;
}

export function useSyncEvents() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const unlisten = listen<RemoteChangePayload>('sync:remote_change', (event) => {
      const { table, scope_type, scope_id } = event.payload;

      switch (table) {
        case 'agents':
          queryClient.invalidateQueries({ queryKey: ['agents'] });
          queryClient.invalidateQueries({ queryKey: ['agent-role-ids'] });
          break;

        case 'tasks':
          queryClient.invalidateQueries({ queryKey: ['tasks'] });
          break;

        case 'schedules':
          queryClient.invalidateQueries({ queryKey: ['schedules'] });
          queryClient.invalidateQueries({ queryKey: ['pulse-config'] });
          break;

        case 'runs':
          queryClient.invalidateQueries({ queryKey: ['runs'] });
          queryClient.invalidateQueries({ queryKey: ['active-runs'] });
          break;

        case 'chat_sessions':
          queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
          break;

        case 'chat_messages':
          queryClient.invalidateQueries({ queryKey: ['chat-messages'] });
          queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
          break;

        case 'agent_conversations':
          queryClient.invalidateQueries({ queryKey: ['agent-conversation'] });
          break;

        case 'chat_compaction_summaries':
          queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
          queryClient.invalidateQueries({ queryKey: ['chat-messages'] });
          break;

        case 'bus_messages':
          queryClient.invalidateQueries({ queryKey: ['bus-messages'] });
          queryClient.invalidateQueries({ queryKey: ['bus-thread'] });
          break;

        case 'bus_subscriptions':
          queryClient.invalidateQueries({ queryKey: ['bus-subscriptions'] });
          break;

        case 'projects':
          queryClient.invalidateQueries({ queryKey: ['projects'] });
          queryClient.invalidateQueries({ queryKey: ['agent-projects'] });
          queryClient.invalidateQueries({ queryKey: ['project-agents'] });
          break;

        case 'project_agents':
          queryClient.invalidateQueries({ queryKey: ['agent-projects'] });
          queryClient.invalidateQueries({ queryKey: ['project-agents'] });
          break;

        case 'workspace_objects':
          if (scope_type === 'agent' && scope_id) {
            queryClient.invalidateQueries({ queryKey: ['workspace-files', scope_id] });
          } else if (scope_type === 'project' && scope_id) {
            queryClient.invalidateQueries({
              queryKey: ['project-workspace-files', scope_id],
            });
          } else {
            // Fallback: invalidate all workspace file caches
            queryClient.invalidateQueries({ queryKey: ['workspace-files'] });
            queryClient.invalidateQueries({ queryKey: ['project-workspace-files'] });
          }
          break;

        default:
          break;
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [queryClient]);
}
