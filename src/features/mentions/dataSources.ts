import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { agentsApi } from '../../api/agents';
import { projectsApi } from '../../api/projects';
import { skillsApi } from '../../api/skills';
import { workItemsApi } from '../../api/workItems';
import { formatWorkItemId } from '../../lib/workItemId';
import { listFilesRecursive } from './listFilesRecursive';
import { MentionGroup, MentionItem, MentionKind } from './types';

function scoreLabel(label: string, query: string): number {
  if (!query) return 1;
  const lowerLabel = label.toLowerCase();
  const lowerQuery = query.toLowerCase();
  const idx = lowerLabel.indexOf(lowerQuery);
  if (idx < 0) return 0;
  if (idx === 0) return 3;
  if (lowerLabel.split(/[/\s._-]/).some((part) => part.startsWith(lowerQuery))) return 2;
  return 1;
}

function rank<T extends { label: string }>(items: T[], query: string, limit = 8): T[] {
  const scored = items
    .map((item) => ({ item, score: scoreLabel(item.label, query) }))
    .filter(({ score }) => score > 0)
    .sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score;
      return a.item.label.localeCompare(b.item.label);
    })
    .slice(0, limit);
  return scored.map(({ item }) => item);
}

export function useAgentsMentionSource(currentAgentId: string | null) {
  return useQuery({
    queryKey: ['agents'],
    queryFn: () => agentsApi.list(),
    staleTime: 30_000,
    select: (agents) => {
      const sorted = [...agents].sort((a, b) => {
        if (a.id === currentAgentId && b.id !== currentAgentId) return -1;
        if (b.id === currentAgentId && a.id !== currentAgentId) return 1;
        return a.name.localeCompare(b.name);
      });
      return sorted;
    },
  });
}

export function useFilesMentionSource(agentId: string | null) {
  return useQuery({
    queryKey: ['workspace-files-flat', agentId],
    queryFn: () => listFilesRecursive(agentId!),
    enabled: Boolean(agentId),
    staleTime: 30_000,
  });
}

export function useWorkItemsMentionSource(projectId: string | null) {
  return useQuery({
    queryKey: ['work-items', projectId],
    queryFn: () => workItemsApi.list(projectId!),
    enabled: Boolean(projectId),
    staleTime: 30_000,
  });
}

export function useProjectBoardsMentionSource(projectId: string | null) {
  return useQuery({
    queryKey: ['project-boards', projectId],
    queryFn: () => projectsApi.listBoards(projectId!),
    enabled: Boolean(projectId),
    staleTime: 30_000,
  });
}

export function useSkillsMentionSource(agentId: string | null) {
  return useQuery({
    queryKey: ['skills', agentId],
    queryFn: () => skillsApi.list(agentId!),
    enabled: Boolean(agentId),
    staleTime: 30_000,
  });
}

interface UseMentionGroupsArgs {
  enabled: boolean;
  query: string;
  currentAgentId: string | null;
  projectId: string | null;
}

export function useMentionGroups({
  enabled,
  query,
  currentAgentId,
  projectId,
}: UseMentionGroupsArgs): MentionGroup[] {
  const agentsQuery = useAgentsMentionSource(currentAgentId);
  const filesQuery = useFilesMentionSource(enabled ? currentAgentId : null);
  const itemsQuery = useWorkItemsMentionSource(enabled ? projectId : null);
  const boardsQuery = useProjectBoardsMentionSource(enabled ? projectId : null);
  const skillsQuery = useSkillsMentionSource(enabled ? currentAgentId : null);

  return useMemo<MentionGroup[]>(() => {
    const groups: MentionGroup[] = [];

    const agentItems: MentionItem[] = (agentsQuery.data ?? []).map((agent) => ({
      id: agent.id,
      label: agent.name,
      secondary: agent.id === currentAgentId ? 'current chat' : agent.description ?? undefined,
      token: { kind: 'agent' as MentionKind, label: agent.name, payload: agent.id },
    }));
    const rankedAgents = rank(agentItems, query);
    if (rankedAgents.length > 0) {
      groups.push({ kind: 'agent', title: 'Agents', items: rankedAgents });
    }

    const skillItems: MentionItem[] = (skillsQuery.data ?? [])
      .filter((skill) => skill.enabled)
      .sort((a, b) => {
        if (a.active !== b.active) return a.active ? -1 : 1;
        return a.name.localeCompare(b.name);
      })
      .map((skill) => ({
        id: skill.name,
        label: skill.name,
        secondary: skill.active
          ? skill.description
            ? `active · ${skill.description}`
            : 'active'
          : skill.description,
        token: { kind: 'skill' as MentionKind, label: skill.name, payload: skill.name },
      }));
    const rankedSkills = rank(skillItems, query);
    if (rankedSkills.length > 0) {
      groups.push({ kind: 'skill', title: 'Skills', items: rankedSkills });
    }

    const fileItems: MentionItem[] = (filesQuery.data?.entries ?? []).map((entry) => ({
      id: entry.path,
      label: entry.path,
      secondary: entry.name,
      token: {
        kind: 'file' as MentionKind,
        label: entry.path,
        payload: `${currentAgentId ?? ''}:${entry.path}`,
      },
    }));
    const rankedFiles = rank(fileItems, query);
    if (rankedFiles.length > 0) {
      groups.push({
        kind: 'file',
        title: 'Files',
        items: rankedFiles,
        truncated: filesQuery.data?.truncated,
      });
    }

    const boardPrefixById = new Map<string, string>();
    for (const board of boardsQuery.data ?? []) {
      boardPrefixById.set(board.id, board.prefix);
    }
    const itemMentions: MentionItem[] = (itemsQuery.data ?? []).map((wi) => {
      const prefix = wi.boardId ? boardPrefixById.get(wi.boardId) ?? null : null;
      const displayId = formatWorkItemId(prefix, wi.id);
      return {
        id: wi.id,
        label: `${displayId} ${wi.title}`,
        secondary: `${wi.status}${wi.kind ? ` · ${wi.kind}` : ''}`,
        token: { kind: 'item' as MentionKind, label: wi.title, payload: wi.id },
      };
    });
    const rankedItems = rank(itemMentions, query);
    if (rankedItems.length > 0) {
      groups.push({ kind: 'item', title: 'Work items', items: rankedItems });
    }

    return groups;
  }, [
    query,
    agentsQuery.data,
    filesQuery.data,
    itemsQuery.data,
    boardsQuery.data,
    skillsQuery.data,
    currentAgentId,
  ]);
}
