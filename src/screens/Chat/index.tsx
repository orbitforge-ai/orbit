import { useState, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, ChevronDown } from "lucide-react";
import * as Select from "@radix-ui/react-select";
import { agentsApi } from "../../api/agents";
import { chatApi } from "../../api/chat";
import { useUiStore } from "../../store/uiStore";
import { ChatSession } from "../../types";
import { SessionList } from "./SessionList";
import { ChatPanel } from "./ChatPanel";

export function ChatScreen() {
  const queryClient = useQueryClient();
  const { pendingChatSessionId, clearPendingChatSession } = useUiStore();
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);

  const { data: agents = [] } = useQuery({
    queryKey: ["agents"],
    queryFn: agentsApi.list,
  });

  // Handle pending chat session from external navigation (e.g. clicking a pulse run)
  useEffect(() => {
    if (!pendingChatSessionId || agents.length === 0) return;

    // Look up the session to find which agent it belongs to
    async function resolve() {
      for (const agent of agents) {
        const sessions: ChatSession[] = await chatApi.listSessions(agent.id, true);
        const match = sessions.find((s) => s.id === pendingChatSessionId);
        if (match) {
          setSelectedAgentId(agent.id);
          setActiveSessionId(match.id);
          clearPendingChatSession();
          return;
        }
      }
      // Session not found — just clear it
      clearPendingChatSession();
    }
    resolve();
  }, [pendingChatSessionId, agents]);

  // Auto-select first agent
  useEffect(() => {
    if (!selectedAgentId && agents.length > 0) {
      setSelectedAgentId(agents[0].id);
    }
  }, [agents, selectedAgentId]);

  async function handleNewSession() {
    if (!selectedAgentId) return;
    const session = await chatApi.createSession(selectedAgentId);
    queryClient.invalidateQueries({ queryKey: ["chat-sessions"] });
    setActiveSessionId(session.id);
  }

  return (
    <div className="flex h-full">
      {/* Left panel: agent selector + session list */}
      <div className="w-[280px] flex flex-col border-r border-edge bg-panel">
        {/* Agent selector */}
        <div className="px-4 py-3 border-b border-edge">
          <label className="text-[10px] uppercase tracking-wider text-muted mb-1.5 block">
            Agent
          </label>
          {agents.length > 0 && selectedAgentId && (
            <Select.Root
              value={selectedAgentId}
              onValueChange={(id) => {
                setSelectedAgentId(id);
                setActiveSessionId(null);
              }}
            >
              <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent">
                <div className="flex items-center gap-2">
                  <Bot size={14} className="text-accent-hover" />
                  <Select.Value />
                </div>
                <Select.Icon>
                  <ChevronDown size={14} className="text-muted" />
                </Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    {agents.map((agent) => (
                      <Select.Item
                        key={agent.id}
                        value={agent.id}
                        className="flex items-center gap-2 px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                      >
                        <Bot size={12} className="text-accent-hover" />
                        <Select.ItemText>{agent.name}</Select.ItemText>
                      </Select.Item>
                    ))}
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
          )}
        </div>

        {/* Session list */}
        {selectedAgentId ? (
          <SessionList
            agentId={selectedAgentId}
            activeSessionId={activeSessionId}
            onSelectSession={setActiveSessionId}
            onNewSession={handleNewSession}
          />
        ) : (
          <div className="flex-1 flex items-center justify-center text-muted text-xs">
            Select an agent to start chatting
          </div>
        )}
      </div>

      {/* Right panel: conversation */}
      <div className="flex-1 min-h-0">
        {activeSessionId ? (
          <ChatPanel sessionId={activeSessionId} />
        ) : (
          <div className="flex items-center justify-center h-full text-muted text-sm">
            Select or create a chat session
          </div>
        )}
      </div>
    </div>
  );
}
