import {
  LayoutDashboard,
  ListChecks,
  History,
  Bot,
  Clock,
  MessageSquare,
  Plus,
} from "lucide-react";
import { cn } from "../lib/cn";
import { useUiStore } from "../store/uiStore";

const NAV_ITEMS = [
  { id: "dashboard" as const, label: "Dashboard", icon: LayoutDashboard },
  { id: "tasks" as const, label: "Tasks", icon: ListChecks },
  { id: "history" as const, label: "Run History", icon: History },
  { id: "agents" as const, label: "Agents", icon: Bot },
  { id: "schedules" as const, label: "Schedules", icon: Clock },
  { id: "chat" as const, label: "Chat", icon: MessageSquare },
];

export function Sidebar() {
  const { screen, navigate } = useUiStore();

  return (
    <aside className="w-[220px] flex-shrink-0 flex flex-col border-r border-[#2a2d3e] bg-[#13151e] h-full">

      {/* Navigation */}
      <nav className="flex-1 px-2 py-3 space-y-0.5 overflow-y-auto">
        {NAV_ITEMS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => navigate(id)}
            className={cn(
              "w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors",
              screen === id
                ? "bg-[#6366f1]/15 text-[#818cf8]"
                : "text-[#94a3b8] hover:bg-[#1a1d27] hover:text-white"
            )}
          >
            <Icon size={16} />
            {label}
          </button>
        ))}
      </nav>

      {/* New Task shortcut */}
      <div className="p-3 border-t border-[#2a2d3e]">
        <button
          onClick={() => navigate("task-builder")}
          className="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] text-white text-sm font-medium transition-colors"
        >
          <Plus size={14} />
          New Task
        </button>
      </div>
    </aside>
  );
}
