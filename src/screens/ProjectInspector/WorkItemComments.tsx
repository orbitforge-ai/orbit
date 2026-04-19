import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { confirm } from '@tauri-apps/plugin-dialog';
import { Bot, Pencil, Trash2, User } from 'lucide-react';
import { workItemsApi } from '../../api/workItems';
import { Agent, WorkItemComment } from '../../types';
import { cn } from '../../lib/cn';

function formatRelative(iso: string): string {
  const then = new Date(iso).getTime();
  const now = Date.now();
  const sec = Math.round((now - then) / 1000);
  if (sec < 60) return 'just now';
  if (sec < 3600) return `${Math.floor(sec / 60)}m ago`;
  if (sec < 86400) return `${Math.floor(sec / 3600)}h ago`;
  if (sec < 86400 * 7) return `${Math.floor(sec / 86400)}d ago`;
  return new Date(iso).toLocaleDateString();
}

export function WorkItemComments({
  workItemId,
  agents,
}: {
  workItemId: string;
  agents: Agent[];
}) {
  const queryClient = useQueryClient();
  const queryKey = ['work-items', workItemId, 'comments'];

  const { data: comments = [], isLoading } = useQuery<WorkItemComment[]>({
    queryKey,
    queryFn: () => workItemsApi.listComments(workItemId),
  });

  const agentById = new Map(agents.map((a) => [a.id, a]));

  const [body, setBody] = useState('');
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editBody, setEditBody] = useState('');

  const createMutation = useMutation({
    mutationFn: (text: string) =>
      workItemsApi.createComment(workItemId, text, { kind: 'user' }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
      setBody('');
    },
  });

  const updateMutation = useMutation({
    mutationFn: ({ id, text }: { id: string; text: string }) =>
      workItemsApi.updateComment(id, text),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey });
      setEditingId(null);
      setEditBody('');
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => workItemsApi.deleteComment(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey }),
  });

  function handleSubmit() {
    const trimmed = body.trim();
    if (!trimmed) return;
    createMutation.mutate(trimmed);
  }

  return (
    <div className="space-y-3">
      {isLoading ? (
        <div className="text-xs text-muted">Loading…</div>
      ) : comments.length === 0 ? (
        <div className="text-xs text-muted italic">No comments yet</div>
      ) : (
        <ul className="space-y-2">
          {comments.map((c) => {
            const isAgent = c.authorKind === 'agent';
            const agent = c.authorAgentId ? agentById.get(c.authorAgentId) ?? null : null;
            const authorName = isAgent ? agent?.name ?? 'Agent' : 'You';
            return (
              <li
                key={c.id}
                className={cn(
                  'rounded-lg border px-3 py-2',
                  isAgent
                    ? 'border-emerald-400/20 bg-emerald-400/5'
                    : 'border-edge bg-surface',
                )}
              >
                <div className="flex items-center justify-between gap-2 mb-1">
                  <div className="flex items-center gap-1.5 text-[11px]">
                    {isAgent ? (
                      <Bot size={11} className="text-emerald-400" />
                    ) : (
                      <User size={11} className="text-muted" />
                    )}
                    <span className="font-medium text-white">{authorName}</span>
                    <span className="text-muted">·</span>
                    <span className="text-muted">{formatRelative(c.createdAt)}</span>
                  </div>
                  {!isAgent && editingId !== c.id && (
                    <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button
                        onClick={() => {
                          setEditingId(c.id);
                          setEditBody(c.body);
                        }}
                        className="p-1 rounded text-muted hover:text-white hover:bg-edge transition-colors"
                        title="Edit"
                      >
                        <Pencil size={10} />
                      </button>
                      <button
                        onClick={async () => {
                          if (!(await confirm('Delete this comment?'))) return;
                          deleteMutation.mutate(c.id);
                        }}
                        className="p-1 rounded text-muted hover:text-red-400 hover:bg-red-400/10 transition-colors"
                        title="Delete"
                      >
                        <Trash2 size={10} />
                      </button>
                    </div>
                  )}
                </div>
                {editingId === c.id ? (
                  <div className="space-y-2">
                    <textarea
                      value={editBody}
                      onChange={(e) => setEditBody(e.target.value)}
                      onKeyDown={(e) => {
                        if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                          updateMutation.mutate({ id: c.id, text: editBody.trim() });
                        } else if (e.key === 'Escape') {
                          setEditingId(null);
                          setEditBody('');
                        }
                      }}
                      rows={3}
                      className="w-full bg-background border border-edge rounded px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
                    />
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => updateMutation.mutate({ id: c.id, text: editBody.trim() })}
                        disabled={!editBody.trim() || updateMutation.isPending}
                        className="px-2.5 py-1 rounded bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-[11px] font-medium transition-colors"
                      >
                        Save
                      </button>
                      <button
                        onClick={() => {
                          setEditingId(null);
                          setEditBody('');
                        }}
                        className="text-[11px] text-muted hover:text-white transition-colors"
                      >
                        Cancel
                      </button>
                    </div>
                  </div>
                ) : (
                  <p className="text-xs text-secondary whitespace-pre-wrap break-words">
                    {c.body}
                  </p>
                )}
              </li>
            );
          })}
        </ul>
      )}

      <div className="space-y-1.5">
        <textarea
          value={body}
          onChange={(e) => setBody(e.target.value)}
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
              handleSubmit();
            }
          }}
          rows={3}
          placeholder="Leave a comment… (Cmd/Ctrl+Enter to submit)"
          className="w-full bg-surface border border-edge rounded-lg px-3 py-2 text-xs text-white placeholder-muted outline-none focus:border-accent"
        />
        <div className="flex justify-end">
          <button
            onClick={handleSubmit}
            disabled={!body.trim() || createMutation.isPending}
            className="px-3 py-1 rounded-md bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium transition-colors"
          >
            {createMutation.isPending ? 'Posting…' : 'Post comment'}
          </button>
        </div>
      </div>
    </div>
  );
}
