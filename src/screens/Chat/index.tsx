import { useState, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot, ChevronDown } from "lucide-react";
import * as Select from "@radix-ui/react-select";
import { agentsApi } from "../../api/agents";
import { chatApi } from "../../api/chat";
import { SessionList } from "./SessionList";
import { ChatPanel } from "./ChatPanel";

export function ChatScreen() {
  const queryClient = useQueryClient();
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);

  const { data: agents = [] } = useQuery({
    queryKey: ["agents"],
    queryFn: agentsApi.list,
  });

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
      <div className="w-[280px] flex flex-col border-r border-[#2a2d3e] bg-[#13151e]">
        {/* Agent selector */}
        <div className="px-4 py-3 border-b border-[#2a2d3e]">
          <label className="text-[10px] uppercase tracking-wider text-[#64748b] mb-1.5 block">
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
              <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]">
                <div className="flex items-center gap-2">
                  <Bot size={14} className="text-[#818cf8]" />
                  <Select.Value />
                </div>
                <Select.Icon>
                  <ChevronDown size={14} className="text-[#64748b]" />
                </Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-[#1a1d27] border border-[#2a2d3e] shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    {agents.map((agent) => (
                      <Select.Item
                        key={agent.id}
                        value={agent.id}
                        className="flex items-center gap-2 px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-[#6366f1]/20"
                      >
                        <Bot size={12} className="text-[#818cf8]" />
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
          <div className="flex-1 flex items-center justify-center text-[#64748b] text-xs">
            Select an agent to start chatting
          </div>
        )}
      </div>

      {/* Right panel: conversation */}
      <div className="flex-1 min-h-0">
        {activeSessionId ? (
          <ChatPanel sessionId={activeSessionId} />
        ) : (
          <div className="flex items-center justify-center h-full text-[#64748b] text-sm">
            Select or create a chat session
          </div>
        )}
      </div>
    </div>
  );
}
