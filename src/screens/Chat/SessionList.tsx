import { useState, useEffect, useRef } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Plus,
  Archive,
  ArchiveRestore,
  Trash2,
  MoreHorizontal,
  Eye,
  MessageSquare,
  Zap,
  GitBranch,
  Loader2,
  CheckCircle2,
  XCircle,
  ChevronDown,
  Bot,
} from 'lucide-react';
import { chatApi } from '../../api/chat';
import { ChatSession } from '../../types';
import { confirm } from '@tauri-apps/plugin-dialog';

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
  const [collapsedSenderGroups, setCollapsedSenderGroups] = useState<Record<string, boolean>>({});
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuSessionId) return;
    function handleClick(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuSessionId(null);
      }
    }
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [menuSessionId]);

  const { data: sessions = [] } = useQuery({
    queryKey: ['chat-sessions', agentId, showArchived],
    queryFn: () => chatApi.listSessions(agentId, showArchived),
    refetchInterval: 5_000,
  });

  async function handleArchive(session: ChatSession) {
    if (session.archived) {
      await chatApi.unarchiveSession(session.id);
    } else {
      await chatApi.archiveSession(session.id);
    }
    queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
    setMenuSessionId(null);
  }

  async function handleDelete(session: ChatSession) {
    if (!(await confirm(`Delete "${session.title}"? This cannot be undone.`))) return;
    await chatApi.deleteSession(session.id);
    queryClient.invalidateQueries({ queryKey: ['chat-sessions'] });
    setMenuSessionId(null);
  }

  function formatTime(dateStr: string) {
    const d = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - d.getTime();
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffDays === 0) return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    if (diffDays === 1) return 'Yesterday';
    if (diffDays < 7) return d.toLocaleDateString([], { weekday: 'short' });
    return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
  }

  function sessionRank(session: ChatSession) {
    if (session.sessionType === 'pulse') return 0;
    if (session.executionState === 'queued' || session.executionState === 'running') return 1;
    if (session.sessionType === 'user_chat') return 3;
    return 2;
  }

  function compareSessions(a: ChatSession, b: ChatSession) {
    const rankDiff = sessionRank(a) - sessionRank(b);
    if (rankDiff !== 0) return rankDiff;
    return new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime();
  }

  const visibleSessions = [...sessions]
    .sort(compareSessions)
    .filter((session) => session.sessionType !== 'sub_agent' || session.executionState === 'queued' || session.executionState === 'running');

  const pulseSessions = visibleSessions.filter((session) => session.sessionType === 'pulse');
  const busSessions = visibleSessions.filter((session) => session.sessionType === 'bus_message');
  const otherAgenticSessions = visibleSessions.filter(
    (session) => session.sessionType !== 'pulse' && session.sessionType !== 'bus_message' && session.sessionType !== 'user_chat'
  );
  const userChats = visibleSessions.filter((session) => session.sessionType === 'user_chat');

  const senderGroups = busSessions.reduce((groups, session) => {
    const senderName = session.sourceAgentName?.trim() || 'Unknown sender';
    const senderId = session.sourceAgentId?.trim() || `unknown:${senderName}`;
    const key = `${senderId}:${senderName}`;
    const existing = groups.get(key);
    if (existing) {
      existing.sessions.push(session);
    } else {
      groups.set(key, {
        key,
        senderId,
        senderName,
        sessions: [session],
      });
    }
    return groups;
  }, new Map<string, { key: string; senderId: string; senderName: string; sessions: ChatSession[] }>());

  const orderedSenderGroups = Array.from(senderGroups.values()).sort((a, b) => {
    const aLatest = Math.max(...a.sessions.map((session) => new Date(session.updatedAt).getTime()));
    const bLatest = Math.max(...b.sessions.map((session) => new Date(session.updatedAt).getTime()));
    return bLatest - aLatest;
  });

  useEffect(() => {
    setCollapsedSenderGroups((prev) => {
      const next: Record<string, boolean> = {};
      for (const group of orderedSenderGroups) {
        const hasActiveSession = group.sessions.some((session) => session.id === activeSessionId);
        const hasRunningSession = group.sessions.some((session) => session.executionState === 'queued' || session.executionState === 'running');
        const defaultCollapsed = orderedSenderGroups.length > 1;
        next[group.key] = prev[group.key] ?? defaultCollapsed;
        if (hasActiveSession || hasRunningSession) {
          next[group.key] = false;
        }
      }
      return next;
    });
  }, [activeSessionId, orderedSenderGroups]);

  function toggleSenderGroup(key: string) {
    setCollapsedSenderGroups((prev) => ({
      ...prev,
      [key]: !prev[key],
    }));
  }

  function renderSessionRow(session: ChatSession) {
    const isPulse = session.sessionType === 'pulse';
    const icon = isPulse
      ? <Zap size={14} className="shrink-0 text-warning" />
      : session.sessionType === 'sub_agent'
        ? <GitBranch size={14} className="shrink-0 text-emerald-400" />
        : session.sessionType === 'bus_message'
          ? <MessageSquare size={14} className="shrink-0 text-blue-400" />
          : <MessageSquare size={14} className="shrink-0 opacity-50" />;

    const stateIcon = session.executionState === 'queued' || session.executionState === 'running'
      ? <Loader2 size={12} className="animate-spin text-accent-hover" />
      : session.executionState === 'success'
        ? <CheckCircle2 size={12} className="text-emerald-400" />
        : session.executionState
          ? <XCircle size={12} className="text-red-400" />
          : null;

    return (
      <div
        id={session.id}
        key={session.id}
        onClick={() => onSelectSession(session.id)}
        className={`group relative flex items-center gap-2.5 px-3 py-2.5 rounded-lg cursor-pointer transition-colors ${
          activeSessionId === session.id
            ? 'bg-accent/15 text-white'
            : 'text-secondary hover:bg-surface hover:text-white'
        }`}
      >
        {icon}
        <div className="flex-1 min-w-0">
          <p className="text-sm truncate">{session.title}</p>
          <p className="text-[10px] text-muted">
            {session.executionState ? `${session.executionState} · ` : ''}
            {formatTime(session.updatedAt)}
          </p>
        </div>
        {stateIcon}

        <button
          onClick={(e) => {
            e.stopPropagation();
            setMenuSessionId(menuSessionId === session.id ? null : session.id);
          }}
          className="opacity-0 group-hover:opacity-100 p-1 rounded text-muted hover:text-white transition-opacity"
        >
          <MoreHorizontal size={14} />
        </button>

        {menuSessionId === session.id && (
          <div
            ref={menuRef}
            className="absolute right-2 top-full mt-1 z-50 rounded-lg bg-surface border border-edge shadow-xl py-1 min-w-[140px]"
            onClick={(e) => e.stopPropagation()}
          >
            <button
              onClick={() => handleArchive(session)}
              className="flex items-center gap-2 w-full px-3 py-1.5 text-xs text-secondary hover:text-white hover:bg-surface-hover"
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
    );
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-edge">
        <h3 className="text-sm font-semibold text-white">Chats</h3>
        <button
          onClick={onNewSession}
          className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
        >
          <Plus size={12} /> New
        </button>
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto p-2 space-y-0.5">
        {visibleSessions.length === 0 && (
          <div className="text-center py-12 text-muted text-xs">
            {showArchived ? 'No archived chats.' : 'No chats yet. Start a new one!'}
          </div>
        )}

        {pulseSessions.map(renderSessionRow)}

        {orderedSenderGroups.map((group) => {
          const isCollapsed = collapsedSenderGroups[group.key] ?? false;
          return (
            <div key={group.key} className="mt-2">
              <button
                onClick={() => toggleSenderGroup(group.key)}
                className="flex items-center gap-2 w-full px-3 py-2 text-left rounded-lg text-xs text-muted hover:text-white hover:bg-surface transition-colors"
              >
                <ChevronDown
                  size={12}
                  className={`transition-transform ${isCollapsed ? '-rotate-90' : ''}`}
                />
                <Bot size={12} className="text-blue-400" />
                <span className="font-medium truncate">{group.senderName}</span>
                <span className="ml-auto text-[10px] text-border-hover">
                  {group.sessions.length}
                </span>
              </button>
              {!isCollapsed && (
                <div className="mt-1 ml-2 space-y-0.5 border-l border-edge pl-2">
                  {group.sessions.map(renderSessionRow)}
                </div>
              )}
            </div>
          );
        })}

        {otherAgenticSessions.map(renderSessionRow)}
        {userChats.map(renderSessionRow)}
      </div>

      {/* Footer: archived toggle */}
      <div className="px-4 py-2 border-t border-edge">
        <button
          onClick={() => setShowArchived(!showArchived)}
          className="flex items-center gap-1.5 text-[10px] text-muted hover:text-secondary transition-colors"
        >
          <Eye size={10} />
          {showArchived ? 'Hide archived' : 'Show archived'}
        </button>
      </div>
    </div>
  );
}
