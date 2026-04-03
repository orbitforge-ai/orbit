import { useEffect } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Sidebar } from './components/Sidebar';
import { useUiStore } from './store/uiStore';
import { useAuthStore } from './store/authStore';
import { useSyncEvents } from './hooks/useSyncEvents';
import { Dashboard } from './screens/Dashboard';
import { RunHistory } from './screens/RunHistory';
import { TaskBuilder } from './screens/TaskBuilder';
import { ScheduleBuilderScreen } from './screens/ScheduleBuilder';
import { TasksScreen } from './screens/Tasks';
import { AgentInspector } from './screens/AgentInspector';
import { TaskEdit } from './screens/TaskEdit';
import { ProjectInspector } from './screens/ProjectInspector';
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
  const { screen } = useUiStore();
  const { state, load } = useAuthStore();

  // Load auth state from Rust backend on first render
  useEffect(() => {
    load();
  }, [load]);

  // Listen for remote Realtime sync events and invalidate React Query caches
  useSyncEvents();

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
      'task-edit': <TaskEdit />,
    } as Record<string, React.ReactNode>
  )[screen] ?? <Dashboard />;

  return (
    <div className="flex h-screen overflow-hidden bg-background">
      <Sidebar />
      <main className="flex-1 overflow-hidden">{content}</main>
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
