import { useEffect } from 'react';
import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query';
import { listen } from '@tauri-apps/api/event';
import { Sidebar } from './components/Sidebar';
import { useUiStore } from './store/uiStore';
import { useAuthStore } from './store/authStore';
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
import { Settings } from './screens/Settings';
import { AuthScreen } from './screens/Auth';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 5_000,
    },
  },
});

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

  // Not yet loaded — render nothing (avoids flash)
  if (state === null) return null;

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
      'task-edit': <TaskEdit />,
    } as Record<string, React.ReactNode>
  )[screen] ?? <Dashboard />;

  return (
    <div className="flex h-screen overflow-hidden bg-background">
      <Sidebar />
      <main className="relative flex-1 overflow-hidden">
        {content}
        {settingsOpen ? <Settings onClose={closeSettings} /> : null}
      </main>
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
