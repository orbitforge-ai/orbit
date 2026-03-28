import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Sidebar } from "./components/Sidebar";
import { useUiStore } from "./store/uiStore";
import { Dashboard } from "./screens/Dashboard";
import { RunHistory } from "./screens/RunHistory";
import { TaskBuilder } from "./screens/TaskBuilder";
import { ScheduleBuilderScreen } from "./screens/ScheduleBuilder";
import { TasksScreen } from "./screens/Tasks";
import { AgentInspector } from "./screens/AgentInspector";
import { TaskEdit } from "./screens/TaskEdit";

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

  const content = (
    {
      dashboard: <Dashboard />,
      history: <RunHistory />,
      "task-builder": <TaskBuilder />,
      "schedule-builder": <ScheduleBuilderScreen />,
      schedules: <ScheduleBuilderScreen />,
      tasks: <TasksScreen />,
      agents: <AgentInspector />,
      "task-edit": <TaskEdit />,
    } as Record<string, React.ReactNode>
  )[screen] ?? <Dashboard />;

  return (
    <div className="flex h-screen overflow-hidden bg-[#0f1117]">
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
