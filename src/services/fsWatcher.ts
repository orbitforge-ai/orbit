import type { QueryClient } from '@tanstack/react-query';
import {
  exists,
  stat,
  watch,
  type FileInfo,
  type UnwatchFn,
  type WatchEvent,
  type WatchEventKind,
} from '@tauri-apps/plugin-fs';
import picomatch from 'picomatch';
import { emitFsTriggerEvent } from '../api/fsWatchTriggers';
import { projectWorkflowsApi } from '../api/projectWorkflows';
import { listen, TRANSPORT_MODE, type UnlistenFn } from '../api/transport';
import type {
  FsWatchEventType,
  FsWatchTriggerConfig,
  ProjectWorkflow,
} from '../types';

const WATCH_DEBOUNCE_MS = 500;
const ATOMIC_RENAME_WINDOW_MS = 250;
const FS_WATCH_TRIGGER_KIND = 'trigger.fs-watch';

export type FsWatchIssueKind = 'workflow:watch-failed' | 'fs:watch-error';

export interface FsWatchIssue {
  workflowId: string;
  path: string;
  message?: string;
  kind: FsWatchIssueKind;
}

type WatcherEntry = {
  key: string;
  workflowId: string;
  watchPath: string;
  watchedPath: string;
  config: FsWatchTriggerConfig;
  unwatch: UnwatchFn;
};

type ResolvedWatchTarget = {
  canonicalKey: string;
  watchPath: string;
  info: FileInfo;
};

type PendingCoalescedEvent = {
  events: FsWatchEventType[];
  timer: number;
};

type QueryCacheEvent = {
  query?: {
    queryKey?: readonly unknown[];
  };
};

export function normalizeFsPath(path: string): string {
  let normalized = path.trim().replace(/\\/g, '/');
  if (normalized.length > 1) {
    normalized = normalized.replace(/\/+$/, '');
  }
  return normalized;
}

export function normalizeFsWatchConfig(value: unknown): FsWatchTriggerConfig {
  const record = isRecord(value) ? value : {};
  return {
    paths: asStringArray(record.paths),
    events: asStringArray(record.events).filter(isFsWatchEventType),
    includeGlobs: asStringArray(record.includeGlobs),
    excludeGlobs: asStringArray(record.excludeGlobs),
  };
}

export function shouldEmitFsWatchEvent(
  config: FsWatchTriggerConfig,
  path: string,
  eventType: FsWatchEventType,
  watchedPath: string,
): boolean {
  if (!config.events.includes(eventType)) {
    return false;
  }

  const includeGlobs = config.includeGlobs?.map((p) => p.trim()).filter(Boolean) ?? [];
  const excludeGlobs = config.excludeGlobs?.map((p) => p.trim()).filter(Boolean) ?? [];
  const normalizedPath = normalizeFsPath(path);
  const relativePath = relativeToWatchedPath(normalizedPath, normalizeFsPath(watchedPath));

  if (includeGlobs.length > 0 && !matchesAny(includeGlobs, normalizedPath, relativePath)) {
    return false;
  }
  if (excludeGlobs.length > 0 && matchesAny(excludeGlobs, normalizedPath, relativePath)) {
    return false;
  }
  return true;
}

export function coalesceFsWatchEvents(
  events: FsWatchEventType[],
  pathExists: boolean,
): FsWatchEventType | null {
  if (events.length === 0) {
    return null;
  }
  const sawDeleteThenCreate = events.some(
    (event, index) =>
      event === 'deleted' && events.slice(index + 1).includes('created'),
  );
  const sawCreateDeleteCreate =
    events[0] === 'created' &&
    events.includes('deleted') &&
    events.lastIndexOf('created') > events.indexOf('deleted');

  if (pathExists && (sawDeleteThenCreate || sawCreateDeleteCreate)) {
    return 'modified';
  }
  if (events.includes('modified')) {
    return 'modified';
  }
  return events[events.length - 1] ?? null;
}

export class FsWatcherManager {
  private started = false;
  private queryCacheUnsubscribe: (() => void) | null = null;
  private cloudUnlisten: Promise<UnlistenFn> | null = null;
  private watchers = new Map<string, WatcherEntry>();
  private issues = new Map<string, FsWatchIssue>();
  private issueSubscribers = new Set<() => void>();
  private pendingEvents = new Map<string, PendingCoalescedEvent>();
  private refreshTimer: number | null = null;
  private reconcileQueue: Promise<void> = Promise.resolve();

  start(queryClient: QueryClient): void {
    if (this.started || TRANSPORT_MODE !== 'tauri') {
      return;
    }
    this.started = true;

    this.queryCacheUnsubscribe = queryClient.getQueryCache().subscribe((event) => {
      if (isProjectWorkflowQueryEvent(event as QueryCacheEvent)) {
        this.scheduleRefresh();
      }
    });

    this.cloudUnlisten = listen('cloud:synced', () => {
      this.scheduleRefresh();
    });
    window.addEventListener('beforeunload', this.handleBeforeUnload);
    this.scheduleRefresh(0);
  }

  async stop(): Promise<void> {
    this.started = false;
    this.queryCacheUnsubscribe?.();
    this.queryCacheUnsubscribe = null;
    this.cloudUnlisten?.then((unlisten) => unlisten()).catch(() => {});
    this.cloudUnlisten = null;
    window.removeEventListener('beforeunload', this.handleBeforeUnload);
    if (this.refreshTimer !== null) {
      window.clearTimeout(this.refreshTimer);
      this.refreshTimer = null;
    }
    for (const pending of this.pendingEvents.values()) {
      window.clearTimeout(pending.timer);
    }
    this.pendingEvents.clear();
    await Promise.all([...this.watchers.values()].map((entry) => this.unwatchEntry(entry)));
    this.watchers.clear();
    this.clearAllIssues();
  }

  subscribeIssues(listener: () => void): () => void {
    this.issueSubscribers.add(listener);
    return () => {
      this.issueSubscribers.delete(listener);
    };
  }

  getIssues(workflowId?: string): FsWatchIssue[] {
    const issues = [...this.issues.values()];
    return workflowId
      ? issues.filter((issue) => issue.workflowId === workflowId)
      : issues;
  }

  private handleBeforeUnload = () => {
    void this.stop();
  };

  private scheduleRefresh(delayMs = 100): void {
    if (!this.started) {
      return;
    }
    if (this.refreshTimer !== null) {
      window.clearTimeout(this.refreshTimer);
    }
    this.refreshTimer = window.setTimeout(() => {
      this.refreshTimer = null;
      void this.refreshFromBackend();
    }, delayMs);
  }

  private async refreshFromBackend(): Promise<void> {
    if (!this.started) {
      return;
    }
    try {
      const workflows = await projectWorkflowsApi.listEnabledTriggers();
      await this.reconcile(workflows);
    } catch (error) {
      console.warn('fs watcher: reconcile failed', error);
    }
  }

  private reconcile(workflows: ProjectWorkflow[]): Promise<void> {
    this.reconcileQueue = this.reconcileQueue
      .then(() => this.reconcileNow(workflows))
      .catch((error) => {
        console.warn('fs watcher: reconcile queue failed', error);
      });
    return this.reconcileQueue;
  }

  private async reconcileNow(workflows: ProjectWorkflow[]): Promise<void> {
    const desiredKeys = new Set<string>();
    const desiredIssueKeys = new Set<string>();

    for (const workflow of workflows) {
      if (!workflow.enabled || workflow.triggerKind !== FS_WATCH_TRIGGER_KIND) {
        continue;
      }
      const config = workflowFsWatchConfig(workflow);
      const configuredPaths = unique(config.paths.map(normalizeFsPath).filter(Boolean));
      const workflowCanonicalKeys = new Set<string>();

      for (const configuredPath of configuredPaths) {
        desiredIssueKeys.add(issueKey(workflow.id, configuredPath));
        let target: ResolvedWatchTarget;
        try {
          target = await this.resolveWatchTarget(configuredPath);
        } catch (error) {
          this.setIssue({
            workflowId: workflow.id,
            path: configuredPath,
            kind: 'workflow:watch-failed',
            message: errorMessage(error, 'Path not available on this device'),
          });
          continue;
        }

        if (workflowCanonicalKeys.has(target.canonicalKey)) {
          continue;
        }
        workflowCanonicalKeys.add(target.canonicalKey);
        const key = `${workflow.id}::${target.canonicalKey}`;
        desiredKeys.add(key);

        const existing = this.watchers.get(key);
        if (existing) {
          existing.config = config;
          existing.watchPath = target.watchPath;
          existing.watchedPath = configuredPath;
          this.clearIssue(workflow.id, configuredPath);
          continue;
        }

        try {
          const unwatch = await watch(
            target.watchPath,
            (event) => {
              void this.handleWatchEvent(key, event);
            },
            { recursive: target.info.isDirectory, delayMs: WATCH_DEBOUNCE_MS },
          );
          this.watchers.set(key, {
            key,
            workflowId: workflow.id,
            watchPath: target.watchPath,
            watchedPath: configuredPath,
            config,
            unwatch,
          });
          this.clearIssue(workflow.id, configuredPath);
        } catch (error) {
          this.setIssue({
            workflowId: workflow.id,
            path: configuredPath,
            kind: 'fs:watch-error',
            message: errorMessage(error, 'Unable to watch this path'),
          });
        }
      }
    }

    await Promise.all(
      [...this.watchers.values()]
        .filter((entry) => !desiredKeys.has(entry.key))
        .map(async (entry) => {
          await this.unwatchEntry(entry);
          this.watchers.delete(entry.key);
        }),
    );

    this.clearStaleIssues(desiredIssueKeys);
  }

  private async resolveWatchTarget(path: string): Promise<ResolvedWatchTarget> {
    const normalized = normalizeFsPath(path);
    const info = await stat(normalized);
    const canonicalKey =
      info.dev !== null && info.ino !== null
        ? `inode:${info.dev}:${info.ino}`
        : `path:${normalized}`;
    return {
      canonicalKey,
      watchPath: normalized,
      info,
    };
  }

  private async unwatchEntry(entry: WatcherEntry): Promise<void> {
    try {
      entry.unwatch();
    } catch (error) {
      console.warn('fs watcher: unwatch failed', error);
    }
  }

  private async handleWatchEvent(watcherKey: string, event: WatchEvent): Promise<void> {
    const entry = this.watchers.get(watcherKey);
    if (!entry) {
      return;
    }
    const eventType = watchEventType(event.type);
    if (!eventType) {
      return;
    }

    for (const rawPath of event.paths) {
      const path = normalizeFsPath(rawPath);
      if (!path) {
        continue;
      }
      this.enqueueCoalescedEvent(entry, path, eventType);
    }
  }

  private enqueueCoalescedEvent(
    entry: WatcherEntry,
    path: string,
    eventType: FsWatchEventType,
  ): void {
    const key = `${entry.key}::${path}`;
    const previous = this.pendingEvents.get(key);
    if (previous) {
      window.clearTimeout(previous.timer);
      previous.events.push(eventType);
      previous.timer = this.createFlushTimer(entry, path, key);
      return;
    }

    this.pendingEvents.set(key, {
      events: [eventType],
      timer: this.createFlushTimer(entry, path, key),
    });
  }

  private createFlushTimer(entry: WatcherEntry, path: string, pendingKey: string): number {
    return window.setTimeout(() => {
      void this.flushCoalescedEvent(entry, path, pendingKey);
    }, ATOMIC_RENAME_WINDOW_MS);
  }

  private async flushCoalescedEvent(
    entry: WatcherEntry,
    path: string,
    pendingKey: string,
  ): Promise<void> {
    const pending = this.pendingEvents.get(pendingKey);
    if (!pending) {
      return;
    }
    this.pendingEvents.delete(pendingKey);

    const pathExists = await exists(path).catch(() => false);
    const eventType = coalesceFsWatchEvents(pending.events, pathExists);
    if (!eventType || !shouldEmitFsWatchEvent(entry.config, path, eventType, entry.watchedPath)) {
      return;
    }

    let isDir = false;
    if (pathExists) {
      isDir = await stat(path)
        .then((info) => info.isDirectory)
        .catch(() => false);
    }

    emitFsTriggerEvent({
      workflowId: entry.workflowId,
      path,
      eventType,
      isDir,
      watchedPath: entry.watchedPath,
    }).catch((error) => {
      console.warn('fs watcher: emit failed', error);
    });
  }

  private setIssue(issue: FsWatchIssue): void {
    const key = issueKey(issue.workflowId, issue.path);
    const previous = this.issues.get(key);
    if (
      previous?.kind === issue.kind &&
      previous.message === issue.message &&
      previous.path === issue.path
    ) {
      return;
    }
    this.issues.set(key, issue);
    dispatchFsWatchIssue(issue);
    this.notifyIssueSubscribers();
  }

  private clearIssue(workflowId: string, path: string): void {
    if (this.issues.delete(issueKey(workflowId, path))) {
      this.notifyIssueSubscribers();
    }
  }

  private clearStaleIssues(desiredIssueKeys: Set<string>): void {
    let changed = false;
    for (const key of [...this.issues.keys()]) {
      if (!desiredIssueKeys.has(key)) {
        this.issues.delete(key);
        changed = true;
      }
    }
    if (changed) {
      this.notifyIssueSubscribers();
    }
  }

  private clearAllIssues(): void {
    if (this.issues.size === 0) {
      return;
    }
    this.issues.clear();
    this.notifyIssueSubscribers();
  }

  private notifyIssueSubscribers(): void {
    for (const listener of this.issueSubscribers) {
      listener();
    }
  }
}

function workflowFsWatchConfig(workflow: ProjectWorkflow): FsWatchTriggerConfig {
  const triggerNode = workflow.graph.nodes.find((node) => node.type === FS_WATCH_TRIGGER_KIND);
  return normalizeFsWatchConfig(triggerNode?.data ?? workflow.triggerConfig);
}

function isProjectWorkflowQueryEvent(event: QueryCacheEvent): boolean {
  const key = event.query?.queryKey;
  return Array.isArray(key) && (key[0] === 'project-workflows' || key[0] === 'project-workflow');
}

function watchEventType(kind: WatchEventKind): FsWatchEventType | null {
  if (typeof kind === 'string') {
    return null;
  }
  if ('create' in kind) {
    return 'created';
  }
  if ('remove' in kind) {
    return 'deleted';
  }
  if ('modify' in kind && kind.modify.kind !== 'rename') {
    return 'modified';
  }
  return null;
}

function matchesAny(patterns: string[], path: string, relativePath: string): boolean {
  const matcher = picomatch(patterns, { dot: true });
  return matcher(path) || Boolean(relativePath && matcher(relativePath));
}

function relativeToWatchedPath(path: string, watchedPath: string): string {
  if (path === watchedPath) {
    return path.split('/').pop() ?? path;
  }
  const prefix = watchedPath.endsWith('/') ? watchedPath : `${watchedPath}/`;
  return path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

function dispatchFsWatchIssue(issue: FsWatchIssue): void {
  window.dispatchEvent(new CustomEvent(issue.kind, { detail: issue }));
}

function issueKey(workflowId: string, path: string): string {
  return `${workflowId}::${normalizeFsPath(path)}`;
}

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : String(error || fallback);
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string').map(normalizeFsPath)
    : [];
}

function isFsWatchEventType(value: string): value is FsWatchEventType {
  return value === 'created' || value === 'modified' || value === 'deleted';
}

function unique(values: string[]): string[] {
  return [...new Set(values)];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

export const fsWatcherManager = new FsWatcherManager();
