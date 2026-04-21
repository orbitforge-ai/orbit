import { useEffect } from 'react';
import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { Sidebar } from './components/Sidebar';
import { useUiStore } from './store/uiStore';
import { useAuthStore } from './store/authStore';
import { useLiveChatStore } from './store/liveChatStore';
import { onPermissionCancelled, onPermissionRequest } from './events/permissionEvents';
import {
  onAgentContentBlock,
  onAgentIteration,
  onAgentLlmChunk,
  onAgentToolResult,
  onMessageReaction,
  onUserQuestion,
} from './events/runEvents';
import { Dashboard } from './screens/Dashboard';
import { RunHistory } from './screens/RunHistory';
import { TaskBuilder } from './screens/TaskBuilder';
import { ScheduleBuilderScreen } from './screens/ScheduleBuilder';
import { TasksScreen } from './screens/Tasks';
import { AgentInspector } from './screens/AgentInspector';
import { TaskEdit } from './screens/TaskEdit';
import { ProjectInspector } from './screens/ProjectInspector';
import { WorkflowEditor } from './screens/WorkflowEditor';
import { Memory } from './screens/Memory';
import { Plugins } from './screens/Plugins';
import { Settings } from './screens/Settings';
import { AuthScreen } from './screens/Auth';
import { BootScreen } from './components/BootScreen';
import { ToastContainer } from './components/Toast';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 5_000,
    },
  },
});

function toChatStreamId(runId: string | null | undefined, sessionId?: string | null) {
  if (runId?.startsWith('chat:')) return runId;
  if (sessionId) return `chat:${sessionId}`;
  return null;
}

function ChatStreamBridge() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const unsubs: Promise<() => void>[] = [];

    unsubs.push(
      onAgentLlmChunk((payload) => {
        if (!payload.runId.startsWith('chat:')) return;
        useLiveChatStore.getState().appendTextDelta(payload.runId, payload.delta);
      })
    );

    unsubs.push(
      onAgentContentBlock((payload) => {
        if (!payload.runId.startsWith('chat:')) return;
        useLiveChatStore.getState().addContentBlock(payload.runId, payload);
      })
    );

    unsubs.push(
      onAgentToolResult((payload) => {
        if (!payload.runId.startsWith('chat:')) return;
        useLiveChatStore
          .getState()
          .addToolResult(payload.runId, payload.toolUseId, payload.content, payload.isError);
      })
    );

    unsubs.push(
      onAgentIteration((payload) => {
        if (!payload.runId.startsWith('chat:')) return;
        useLiveChatStore.getState().handleIteration(payload.runId, payload);
        if (payload.action === 'finished') {
          const sessionId = payload.runId.slice(5);
          queryClient.invalidateQueries({ queryKey: ['chat-messages', sessionId] });
          queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
          queryClient.invalidateQueries({ queryKey: ['chat-session-execution', sessionId] });
          queryClient.invalidateQueries({ queryKey: ['message-reactions', sessionId] });
        }
      })
    );

    unsubs.push(
      onPermissionRequest((payload) => {
        const streamId = toChatStreamId(payload.runId, payload.sessionId);
        if (!streamId) return;
        useLiveChatStore.getState().addPermissionPrompt(streamId, payload);
      })
    );

    unsubs.push(
      onPermissionCancelled((_payload) => {
        // Pending request visibility is handled by PermissionStore in Sidebar.
      })
    );

    unsubs.push(
      onUserQuestion((payload) => {
        const streamId = toChatStreamId(payload.runId, payload.sessionId);
        if (!streamId) return;
        useLiveChatStore.getState().addUserQuestionPrompt(streamId, payload);
      })
    );

    unsubs.push(
      onMessageReaction((payload) => {
        const streamId = `chat:${payload.sessionId}`;
        useLiveChatStore.getState().addReaction(streamId, payload);
      })
    );

    return () => {
      unsubs.forEach((unsub) => {
        unsub.then((cleanup) => cleanup()).catch(() => {});
      });
    };
  }, [queryClient]);

  return null;
}

function AppContent() {
  const { screen, settingsOpen, closeSettings } = useUiStore();
  const { state, load } = useAuthStore();
  const queryClient = useQueryClient();

  // Load auth state from Rust backend on first render
  useEffect(() => {
    load();
  }, [load]);

  // When the backend finishes a startup cloud pull, invalidate all cached
  // queries so every screen refetches with the freshly synced data.
  useEffect(() => {
    const unlisten = listen('cloud:synced', () => {
      queryClient.invalidateQueries();
    });
    return () => { unlisten.then(f => f()); };
  }, [queryClient]);

  if (state === null) return <BootScreen />;

  // Show auth screen on first launch or after logout
  if (state.mode === 'unset') return <AuthScreen />;

  const content = (
    {
      dashboard: <Dashboard />,
      history: <RunHistory />,
      'task-builder': <TaskBuilder />,
      'schedule-builder': <ScheduleBuilderScreen />,
      schedules: <ScheduleBuilderScreen />,
      tasks: <TasksScreen />,
      agents: <AgentInspector />,
      projects: <ProjectInspector />,
      'workflow-editor': <WorkflowEditor />,
      memory: <Memory />,
      plugins: <Plugins />,
      'task-edit': <TaskEdit />,
    } as Record<string, React.ReactNode>
  )[screen] ?? <Dashboard />;

  return (
    <div className="flex h-screen overflow-hidden bg-background">
      <ChatStreamBridge />
      <Sidebar />
      <main className="relative flex-1 overflow-hidden">
        {content}
        {settingsOpen ? <Settings onClose={closeSettings} /> : null}
      </main>
      <ToastContainer />
    </div>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AppContent />
    </QueryClientProvider>
  );
}
