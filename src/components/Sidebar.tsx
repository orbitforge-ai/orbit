import { useState, useEffect } from "react";
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
  Shield,
} from "lucide-react";
import { cn } from "../lib/cn";
import { useUiStore } from "../store/uiStore";
import { agentsApi } from "../api/agents";
import { Agent } from "../types";
import { usePermissionStore } from "../store/permissionStore";
import { onPermissionRequest, onPermissionCancelled } from "../events/permissionEvents";

const NAV_ITEMS = [
  { id: "dashboard" as const, label: "Dashboard", icon: LayoutDashboard },
  { id: "tasks" as const, label: "Tasks", icon: ListChecks },
  { id: "history" as const, label: "Run History", icon: History },
  { id: "schedules" as const, label: "Schedules", icon: Clock },
];

export function Sidebar() {
  const { screen, selectedAgentId, navigate, selectAgent, openAgentChat } = useUiStore();
  const [agentsOpen, setAgentsOpen] = useState(screen === "agents");
  const pendingCount = usePermissionStore((s) => s.pendingCount);

  // Global listener for permission events (so badge works even when chat panel isn't open)
  useEffect(() => {
    const unsubs: Promise<() => void>[] = [];
    unsubs.push(
      onPermissionRequest((payload) => {
        usePermissionStore.getState().addRequest(payload);
      })
    );
    unsubs.push(
      onPermissionCancelled((payload) => {
        usePermissionStore.getState().removeRequest(payload.requestId);
      })
    );
    return () => { unsubs.forEach((p) => p.then((fn) => fn())); };
  }, []);

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
            {pendingCount > 0 && (
              <span className="flex items-center gap-1 px-1.5 py-0.5 rounded-full bg-amber-500/20 text-amber-400 text-[10px] font-medium">
                <Shield size={8} />
                {pendingCount}
              </span>
            )}
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
                <div
                  key={agent.id}
                  className={cn(
                    "group flex items-center gap-1 rounded-md transition-colors",
                    screen === "agents" && selectedAgentId === agent.id
                      ? "bg-accent/10 text-accent-hover"
                      : "text-secondary hover:bg-surface hover:text-white"
                  )}
                >
                  <button
                    onClick={() => selectAgent(agent.id)}
                    className="flex min-w-0 flex-1 items-center gap-2 px-2.5 py-1.5 text-xs font-medium truncate"
                  >
                    <span
                      className={cn(
                        "w-1.5 h-1.5 rounded-full shrink-0",
                        agent.state === "idle" ? "bg-emerald-400" : "bg-text-muted"
                      )}
                    />
                    <span className="truncate">{agent.name}</span>
                  </button>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      openAgentChat(agent.id);
                    }}
                    className="mr-1 rounded p-1 text-muted opacity-0 transition-opacity hover:bg-background/60 hover:text-white group-hover:opacity-100"
                    title={`Open ${agent.name} chat`}
                    aria-label={`Open ${agent.name} chat`}
                  >
                    <MessageSquare size={12} />
                  </button>
                </div>
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
