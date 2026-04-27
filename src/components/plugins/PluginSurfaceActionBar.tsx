import { useCallback, useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { listen } from '../../api/transport';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { ChevronRight, Loader2, MoreHorizontal, X } from 'lucide-react';
import {
  pluginsApi,
  type PluginSurface,
  type PluginSurfaceAction,
  type PluginSurfaceActionItem,
  type SurfaceActionPromptField,
} from '../../api/plugins';
import { cn } from '../../lib/cn';
import { Input } from '../ui';
import { toast } from '../../store/toastStore';

interface PluginSurfaceActionBarProps {
  surface: PluginSurface;
  path: string | null;
  variant: 'sidebar' | 'workspace';
  maxInlineActions?: number;
  className?: string;
  onActionComplete?: () => void;
}

interface PromptRequest {
  pluginId: string;
  pluginName: string;
  actionId: string;
  actionLabel: string;
  itemLabel: string;
  tool: string;
  baseArgs: Record<string, unknown>;
  target: PluginSurfaceActionItem['target'];
  fields: SurfaceActionPromptField[];
}

export function PluginSurfaceActionBar({
  surface,
  path,
  variant,
  maxInlineActions = 3,
  className,
  onActionComplete,
}: PluginSurfaceActionBarProps) {
  const queryClient = useQueryClient();
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [promptRequest, setPromptRequest] = useState<PromptRequest | null>(null);

  useEffect(() => {
    const unlistenChanged = listen('plugins:changed', () => {
      queryClient.invalidateQueries({ queryKey: ['plugin-surface-actions'] });
    });
    return () => {
      unlistenChanged.then((u) => u()).catch(() => {});
    };
  }, [queryClient]);

  const actionsQuery = useQuery<PluginSurfaceAction[]>({
    queryKey: ['plugin-surface-actions', surface, path],
    queryFn: () => pluginsApi.listSurfaceActions(surface, path),
    refetchOnWindowFocus: false,
  });

  const actions = actionsQuery.data ?? [];
  const visibleActions = useMemo(
    () => actions.slice(0, maxInlineActions),
    [actions, maxInlineActions]
  );
  const overflowActions = useMemo(
    () => actions.slice(maxInlineActions),
    [actions, maxInlineActions]
  );
  const isRefreshing = actionsQuery.isFetching && actions.length > 0;
  const hasStaleActions = actions.some((action) => action.stale);

  const runItem = useCallback(
    async (
      pluginId: string,
      label: string,
      actionId: string,
      item: Pick<PluginSurfaceActionItem, 'tool' | 'args' | 'target'>
    ) => {
      try {
        setPendingId(actionId);
        await pluginsApi.runSurfaceAction(pluginId, item.tool, item.args, surface, item.target);
        toast.success(label);
        queryClient.invalidateQueries({ queryKey: ['plugin-surface-actions'] });
        onActionComplete?.();
      } catch (err) {
        toast.error(`Failed to run ${label}`, err);
      } finally {
        setPendingId(null);
      }
    },
    [onActionComplete, queryClient, surface]
  );

  const requestRun = useCallback(
    (
      pluginId: string,
      pluginName: string,
      actionLabel: string,
      itemLabel: string,
      actionId: string,
      item: Pick<PluginSurfaceActionItem, 'tool' | 'args' | 'target' | 'prompt'>
    ) => {
      if (item.prompt && item.prompt.length > 0) {
        setPromptRequest({
          pluginId,
          pluginName,
          actionId,
          actionLabel,
          itemLabel,
          tool: item.tool,
          baseArgs: item.args ?? {},
          target: item.target,
          fields: item.prompt,
        });
        return Promise.resolve();
      }
      return runItem(pluginId, itemLabel, actionId, {
        tool: item.tool,
        args: item.args ?? {},
        target: item.target,
      });
    },
    [runItem]
  );

  const handlePromptSubmit = useCallback(
    async (values: Record<string, string>) => {
      if (!promptRequest) return;
      const args = { ...promptRequest.baseArgs, ...values };
      await runItem(promptRequest.pluginId, promptRequest.itemLabel, promptRequest.actionId, {
        tool: promptRequest.tool,
        args,
        target: promptRequest.target,
      });
      setPromptRequest(null);
    },
    [promptRequest, runItem]
  );

  if (actions.length === 0 && !actionsQuery.isLoading && !actionsQuery.isFetching) {
    return null;
  }

  const wrapperClassName =
    variant === 'workspace'
      ? 'flex items-center gap-1 shrink-0'
      : 'flex items-center gap-1.5 min-w-0';

  return (
    <div className={cn(wrapperClassName, className)}>
      {actions.length === 0 && (actionsQuery.isLoading || actionsQuery.isFetching) ? (
        <div className="flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-muted">
          <Loader2 size={11} className="animate-spin" />
          <span>Loading actions…</span>
        </div>
      ) : null}

      {visibleActions.map((action) =>
        action.presentation === 'menu' ? (
          <SurfaceMenuAction
            key={action.id}
            action={action}
            pendingId={pendingId}
            variant={variant}
            onRun={(itemId, item) =>
              requestRun(action.pluginId, action.pluginName, action.label, item.label, itemId, item)
            }
          />
        ) : (
          <SurfaceButtonAction
            key={action.id}
            action={action}
            pending={pendingId === action.id}
            variant={variant}
            onRun={() => {
              if (!action.tool || !action.target) return Promise.resolve();
              return requestRun(
                action.pluginId,
                action.pluginName,
                action.label,
                action.label,
                action.id,
                {
                  tool: action.tool,
                  args: action.args ?? {},
                  target: action.target,
                  prompt: action.prompt,
                }
              );
            }}
          />
        )
      )}

      {overflowActions.length > 0 ? (
        <OverflowMenu
          actions={overflowActions}
          pendingId={pendingId}
          variant={variant}
          onRun={(pluginId, pluginName, actionLabel, itemLabel, itemId, item) =>
            requestRun(pluginId, pluginName, actionLabel, itemLabel, itemId, item)
          }
        />
      ) : null}

      {promptRequest ? (
        <SurfaceActionPromptDialog
          request={promptRequest}
          busy={pendingId === promptRequest.actionId}
          onCancel={() => setPromptRequest(null)}
          onSubmit={handlePromptSubmit}
        />
      ) : null}

      {isRefreshing ? (
        <span
          className="shrink-0 text-muted"
          title="Refreshing plugin actions"
          aria-label="Refreshing plugin actions"
        >
          {/* <Loader2 size={11} className="animate-spin" /> */}
        </span>
      ) : null}

      {hasStaleActions ? (
        <span
          className="shrink-0 text-[10px] text-amber-300"
          title="Showing the last good plugin actions while refresh recovers"
          aria-label="Showing stale plugin actions"
        >
          stale
        </span>
      ) : null}
    </div>
  );
}

function SurfaceButtonAction({
  action,
  pending,
  variant,
  onRun,
}: {
  action: PluginSurfaceAction;
  pending: boolean;
  variant: 'sidebar' | 'workspace';
  onRun: () => Promise<void>;
}) {
  return (
    <button
      onClick={() => {
        void onRun();
      }}
      disabled={pending || action.disabled || !action.tool || !action.target}
      title={action.tooltip ?? `${action.pluginName}: ${action.label}`}
      aria-label={action.label}
      className={buttonClassName(variant, action.hideLabel)}
    >
      {pending ? <Loader2 size={11} className="animate-spin shrink-0" /> : null}
      <ActionTriggerContent action={action} />
    </button>
  );
}

function SurfaceMenuAction({
  action,
  pendingId,
  variant,
  onRun,
}: {
  action: PluginSurfaceAction;
  pendingId: string | null;
  variant: 'sidebar' | 'workspace';
  onRun: (itemId: string, item: PluginSurfaceActionItem) => Promise<void>;
}) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          disabled={action.disabled}
          title={action.tooltip ?? `${action.pluginName}: ${action.label}`}
          aria-label={action.label}
          className={buttonClassName(variant, action.hideLabel)}
        >
          <ActionTriggerContent action={action} showMenuChevron />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          side={variant === 'workspace' ? 'bottom' : 'top'}
          align="end"
          sideOffset={6}
          className="z-50 min-w-44 rounded-xl border border-edge bg-surface p-1.5 shadow-xl"
        >
          {action.items.map((item) => (
            <DropdownMenu.Item
              key={item.id}
              disabled={item.disabled || pendingId === item.id}
              onSelect={() => {
                void onRun(item.id, item);
              }}
              className="flex items-center gap-2 rounded-lg px-2.5 py-2 text-sm text-secondary outline-none cursor-pointer data-[highlighted]:bg-accent/10 data-[highlighted]:text-white data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50"
            >
              {pendingId === item.id ? <Loader2 size={12} className="animate-spin" /> : null}
              <span className="truncate">{item.label}</span>
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function OverflowMenu({
  actions,
  pendingId,
  variant,
  onRun,
}: {
  actions: PluginSurfaceAction[];
  pendingId: string | null;
  variant: 'sidebar' | 'workspace';
  onRun: (
    pluginId: string,
    pluginName: string,
    actionLabel: string,
    itemLabel: string,
    itemId: string,
    item: Pick<PluginSurfaceActionItem, 'tool' | 'args' | 'target' | 'prompt'>
  ) => Promise<void>;
}) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          className={buttonClassName(variant)}
          title="More plugin actions"
          aria-label="More plugin actions"
        >
          <MoreHorizontal size={12} className="shrink-0" />
          <span>More</span>
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          side={variant === 'workspace' ? 'bottom' : 'top'}
          align="end"
          sideOffset={6}
          className="z-50 min-w-48 rounded-xl border border-edge bg-surface p-1.5 shadow-xl"
        >
          {actions.map((action) =>
            action.presentation === 'menu' ? (
              <DropdownMenu.Sub key={action.id}>
                <DropdownMenu.SubTrigger className="flex items-center justify-between gap-2 rounded-lg px-2.5 py-2 text-sm text-secondary outline-none data-[highlighted]:bg-accent/10 data-[highlighted]:text-white">
                  <span className="truncate">{action.label}</span>
                  <ChevronRight size={12} className="shrink-0" />
                </DropdownMenu.SubTrigger>
                <DropdownMenu.Portal>
                  <DropdownMenu.SubContent className="z-50 min-w-44 rounded-xl border border-edge bg-surface p-1.5 shadow-xl">
                    {action.items.map((item) => (
                      <DropdownMenu.Item
                        key={item.id}
                        disabled={item.disabled || pendingId === item.id}
                        onSelect={() => {
                          void onRun(
                            action.pluginId,
                            action.pluginName,
                            action.label,
                            item.label,
                            item.id,
                            item
                          );
                        }}
                        className="flex items-center gap-2 rounded-lg px-2.5 py-2 text-sm text-secondary outline-none cursor-pointer data-[highlighted]:bg-accent/10 data-[highlighted]:text-white data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50"
                      >
                        {pendingId === item.id ? (
                          <Loader2 size={12} className="animate-spin" />
                        ) : null}
                        <span className="truncate">{item.label}</span>
                      </DropdownMenu.Item>
                    ))}
                  </DropdownMenu.SubContent>
                </DropdownMenu.Portal>
              </DropdownMenu.Sub>
            ) : (
              <DropdownMenu.Item
                key={action.id}
                disabled={
                  action.disabled || pendingId === action.id || !action.tool || !action.target
                }
                onSelect={() => {
                  if (!action.tool || !action.target) return;
                  void onRun(
                    action.pluginId,
                    action.pluginName,
                    action.label,
                    action.label,
                    action.id,
                    {
                      tool: action.tool,
                      args: action.args ?? {},
                      target: action.target,
                      prompt: action.prompt,
                    }
                  );
                }}
                className="flex items-center gap-2 rounded-lg px-2.5 py-2 text-sm text-secondary outline-none cursor-pointer data-[highlighted]:bg-accent/10 data-[highlighted]:text-white data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50"
              >
                {pendingId === action.id ? <Loader2 size={12} className="animate-spin" /> : null}
                <span className="truncate">{action.label}</span>
              </DropdownMenu.Item>
            )
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function buttonClassName(variant: 'sidebar' | 'workspace', compact?: boolean) {
  return cn(
    'inline-flex min-w-0 items-center gap-1.5 rounded-md border border-edge px-2 py-1 text-xs text-secondary transition-colors hover:bg-surface hover:text-white disabled:cursor-not-allowed disabled:opacity-50',
    variant === 'sidebar' ? 'max-w-full' : 'shrink-0',
    compact ? 'px-1.5' : null
  );
}

function ActionTriggerContent({
  action,
  showMenuChevron = false,
}: {
  action: PluginSurfaceAction;
  showMenuChevron?: boolean;
}) {
  return (
    <>
      {action.icon ? <ActionIcon icon={action.icon} /> : null}
      {action.hideLabel ? null : <span className="truncate">{action.label}</span>}
      {showMenuChevron ? <ChevronRight size={11} className="shrink-0 rotate-90" /> : null}
    </>
  );
}

function ActionIcon({ icon }: { icon: string }) {
  if (icon === 'github') {
    return (
      <svg viewBox="0 0 16 16" aria-hidden="true" className="h-3.5 w-3.5 shrink-0 fill-current">
        <path d="M8 0C3.58 0 0 3.58 0 8a8 8 0 0 0 5.47 7.59c.4.07.55-.17.55-.38c0-.19-.01-.82-.01-1.49c-2.01.37-2.53-.49-2.69-.94c-.09-.23-.48-.94-.82-1.13c-.28-.15-.68-.52-.01-.53c.63-.01 1.08.58 1.23.82c.72 1.21 1.87.87 2.33.66c.07-.52.28-.87.5-1.07c-1.78-.2-3.64-.89-3.64-3.95c0-.87.31-1.59.82-2.15c-.08-.2-.36-1.02.08-2.12c0 0 .67-.21 2.2.82a7.7 7.7 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82c.44 1.1.16 1.92.08 2.12c.51.56.82 1.27.82 2.15c0 3.07-1.87 3.75-3.65 3.95c.29.25.54.73.54 1.48c0 1.07-.01 1.93-.01 2.2c0 .21.15.46.55.38A8 8 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
      </svg>
    );
  }

  return null;
}

function SurfaceActionPromptDialog({
  request,
  busy,
  onCancel,
  onSubmit,
}: {
  request: PromptRequest;
  busy: boolean;
  onCancel: () => void;
  onSubmit: (values: Record<string, string>) => Promise<void>;
}) {
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(request.fields.map((f) => [f.name, '']))
  );

  const canSubmit =
    !busy &&
    request.fields.every((f) =>
      (f.required ?? true) ? (values[f.name] ?? '').trim() !== '' : true
    );

  function submit() {
    if (!canSubmit) return;
    const trimmed: Record<string, string> = {};
    for (const f of request.fields) {
      const v = (values[f.name] ?? '').trim();
      if (v !== '') trimmed[f.name] = v;
    }
    void onSubmit(trimmed);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="w-[440px] rounded-2xl border border-edge bg-panel shadow-2xl">
        <div className="flex items-center justify-between px-5 py-3 border-b border-edge">
          <h3 className="text-sm font-semibold text-white">
            {request.pluginName}: {request.itemLabel}
          </h3>
          <button
            onClick={onCancel}
            className="p-1 rounded text-muted hover:text-white hover:bg-edge"
            aria-label="Close"
          >
            <X size={14} />
          </button>
        </div>
        <div className="px-5 py-4 space-y-3">
          {request.fields.map((field, i) => (
            <div key={field.name}>
              <label className="text-xs text-muted mb-1 block">{field.label}</label>
              <Input
                value={values[field.name] ?? ''}
                onChange={(e) => setValues((prev) => ({ ...prev, [field.name]: e.target.value }))}
                placeholder={field.placeholder}
                autoFocus={i === 0}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') submit();
                  if (e.key === 'Escape') onCancel();
                }}
              />
              {field.description ? (
                <p className="mt-1 text-[11px] text-muted">{field.description}</p>
              ) : null}
            </div>
          ))}
        </div>
        <div className="flex items-center justify-end gap-2 px-5 py-3 border-t border-edge">
          <button
            onClick={onCancel}
            className="px-3 py-1.5 rounded-lg text-muted hover:text-white text-sm"
          >
            Cancel
          </button>
          <button
            onClick={submit}
            disabled={!canSubmit}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-sm font-medium transition-colors"
          >
            {busy ? <Loader2 size={12} className="animate-spin" /> : null}
            Run
          </button>
        </div>
      </div>
    </div>
  );
}
