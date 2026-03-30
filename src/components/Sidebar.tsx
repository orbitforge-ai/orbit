import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  LayoutDashboard,
  ListChecks,
  History,
  Bot,
  Clock,
  MessageSquare,
  Plus,
  ChevronRight,
} from "lucide-react";
import { cn } from "../lib/cn";
import { useUiStore } from "../store/uiStore";
import { agentsApi } from "../api/agents";
import { Agent } from "../types";

const NAV_ITEMS = [
  { id: "dashboard" as const, label: "Dashboard", icon: LayoutDashboard },
  { id: "tasks" as const, label: "Tasks", icon: ListChecks },
  { id: "history" as const, label: "Run History", icon: History },
  { id: "schedules" as const, label: "Schedules", icon: Clock },
  { id: "chat" as const, label: "Chat", icon: MessageSquare },
];

export function Sidebar() {
  const { screen, selectedAgentId, navigate, selectAgent } = useUiStore();
  const [agentsOpen, setAgentsOpen] = useState(screen === "agents");

  const { data: agents = [] } = useQuery<Agent[]>({
    queryKey: ["agents"],
    queryFn: agentsApi.list,
    refetchInterval: 10_000,
  });

  return (
    <aside className="w-[220px] flex-shrink-0 flex flex-col border-r border-edge bg-panel h-full">

      {/* Navigation */}
      <nav className="flex-1 px-2 py-3 space-y-0.5 overflow-y-auto">
        {NAV_ITEMS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => navigate(id)}
            className={cn(
              "w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors",
              screen === id
                ? "bg-accent/15 text-accent-hover"
                : "text-secondary hover:bg-surface hover:text-white"
            )}
          >
            <Icon size={16} />
            {label}
          </button>
        ))}

        {/* Agents collapsible section */}
        <div>
          <button
            onClick={() => setAgentsOpen(!agentsOpen)}
            className={cn(
              "w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors",
              screen === "agents"
                ? "bg-accent/15 text-accent-hover"
                : "text-secondary hover:bg-surface hover:text-white"
            )}
          >
            <Bot size={16} />
            <span className="flex-1 text-left">Agents</span>
            <ChevronRight
              size={14}
              className={cn(
                "transition-transform text-muted",
                agentsOpen && "rotate-90"
              )}
            />
          </button>

          {agentsOpen && (
            <div className="ml-3 mt-0.5 space-y-0.5 border-l border-edge pl-2">
              {agents.map((agent) => (
                <button
                  key={agent.id}
                  onClick={() => selectAgent(agent.id)}
                  className={cn(
                    "w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors truncate",
                    screen === "agents" && selectedAgentId === agent.id
                      ? "bg-accent/10 text-accent-hover"
                      : "text-secondary hover:bg-surface hover:text-white"
                  )}
                >
                  <span
                    className={cn(
                      "w-1.5 h-1.5 rounded-full shrink-0",
                      agent.state === "idle" ? "bg-emerald-400" : "bg-text-muted"
                    )}
                  />
                  <span className="truncate">{agent.name}</span>
                </button>
              ))}

              {/* New Agent link */}
              <button
                onClick={() => {
                  selectAgent("__new__");
                }}
                className="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs font-medium text-muted hover:text-accent-hover hover:bg-accent/10 transition-colors"
              >
                <Plus size={12} />
                <span>New Agent</span>
              </button>
            </div>
          )}
        </div>
      </nav>

      {/* New Task shortcut */}
      <div className="p-3 border-t border-edge">
        <button
          onClick={() => navigate("task-builder")}
          className="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-lg bg-accent hover:bg-accent-hover text-white text-sm font-medium transition-colors"
        >
          <Plus size={14} />
          New Task
        </button>
      </div>
    </aside>
  );
}
