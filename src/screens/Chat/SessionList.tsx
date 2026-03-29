import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  Archive,
  ArchiveRestore,
  Trash2,
  MoreHorizontal,
  Eye,
  MessageSquare,
} from "lucide-react";
import { chatApi } from "../../api/chat";
import { ChatSession } from "../../types";
import { confirm } from "@tauri-apps/plugin-dialog";

interface SessionListProps {
  agentId: string;
  activeSessionId: string | null;
  onSelectSession: (id: string) => void;
  onNewSession: () => void;
}

export function SessionList({
  agentId,
  activeSessionId,
  onSelectSession,
  onNewSession,
}: SessionListProps) {
  const queryClient = useQueryClient();
  const [showArchived, setShowArchived] = useState(false);
  const [menuSessionId, setMenuSessionId] = useState<string | null>(null);

  const { data: sessions = [] } = useQuery({
    queryKey: ["chat-sessions", agentId, showArchived],
    queryFn: () => chatApi.listSessions(agentId, showArchived),
    refetchInterval: 5_000,
  });

  async function handleArchive(session: ChatSession) {
    if (session.archived) {
      await chatApi.unarchiveSession(session.id);
    } else {
      await chatApi.archiveSession(session.id);
    }
    queryClient.invalidateQueries({ queryKey: ["chat-sessions"] });
    setMenuSessionId(null);
  }

  async function handleDelete(session: ChatSession) {
    if (!(await confirm(`Delete "${session.title}"? This cannot be undone.`))) return;
    await chatApi.deleteSession(session.id);
    queryClient.invalidateQueries({ queryKey: ["chat-sessions"] });
    setMenuSessionId(null);
  }

  function formatTime(dateStr: string) {
    const d = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - d.getTime();
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffDays === 0) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (diffDays === 1) return "Yesterday";
    if (diffDays < 7) return d.toLocaleDateString([], { weekday: "short" });
    return d.toLocaleDateString([], { month: "short", day: "numeric" });
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-[#2a2d3e]">
        <h3 className="text-sm font-semibold text-white">Chats</h3>
        <button
          onClick={onNewSession}
          className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] text-white text-xs font-medium transition-colors"
        >
          <Plus size={12} /> New
        </button>
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto p-2 space-y-0.5">
        {sessions.length === 0 && (
          <div className="text-center py-12 text-[#64748b] text-xs">
            {showArchived ? "No archived chats." : "No chats yet. Start a new one!"}
          </div>
        )}

        {sessions.map((session) => (
          <div
            key={session.id}
            onClick={() => onSelectSession(session.id)}
            className={`group relative flex items-center gap-2.5 px-3 py-2.5 rounded-lg cursor-pointer transition-colors ${
              activeSessionId === session.id
                ? "bg-[#6366f1]/15 text-white"
                : "text-[#94a3b8] hover:bg-[#1a1d27] hover:text-white"
            }`}
          >
            <MessageSquare size={14} className="shrink-0 opacity-50" />
            <div className="flex-1 min-w-0">
              <p className="text-sm truncate">{session.title}</p>
              <p className="text-[10px] text-[#64748b]">{formatTime(session.updatedAt)}</p>
            </div>

            {/* Context menu trigger */}
            <button
              onClick={(e) => {
                e.stopPropagation();
                setMenuSessionId(menuSessionId === session.id ? null : session.id);
              }}
              className="opacity-0 group-hover:opacity-100 p-1 rounded text-[#64748b] hover:text-white transition-opacity"
            >
              <MoreHorizontal size={14} />
            </button>

            {/* Dropdown menu */}
            {menuSessionId === session.id && (
              <div
                className="absolute right-2 top-full mt-1 z-50 rounded-lg bg-[#1a1d27] border border-[#2a2d3e] shadow-xl py-1 min-w-[140px]"
                onClick={(e) => e.stopPropagation()}
              >
                <button
                  onClick={() => handleArchive(session)}
                  className="flex items-center gap-2 w-full px-3 py-1.5 text-xs text-[#94a3b8] hover:text-white hover:bg-[#222533]"
                >
                  {session.archived ? (
                    <>
                      <ArchiveRestore size={12} /> Unarchive
                    </>
                  ) : (
                    <>
                      <Archive size={12} /> Archive
                    </>
                  )}
                </button>
                <button
                  onClick={() => handleDelete(session)}
                  className="flex items-center gap-2 w-full px-3 py-1.5 text-xs text-red-400 hover:bg-red-500/10"
                >
                  <Trash2 size={12} /> Delete
                </button>
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Footer: archived toggle */}
      <div className="px-4 py-2 border-t border-[#2a2d3e]">
        <button
          onClick={() => setShowArchived(!showArchived)}
          className="flex items-center gap-1.5 text-[10px] text-[#64748b] hover:text-[#94a3b8] transition-colors"
        >
          <Eye size={10} />
          {showArchived ? "Hide archived" : "Show archived"}
        </button>
      </div>
    </div>
  );
}
