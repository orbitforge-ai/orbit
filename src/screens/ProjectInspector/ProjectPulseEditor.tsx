import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import * as Switch from '@radix-ui/react-switch';
import { Clock, ExternalLink, Play, Save, X, Zap } from 'lucide-react';
import { pulseApi, PulseConfig } from '../../api/pulse';
import { tasksApi } from '../../api/tasks';
import { Textarea } from '../../components/ui';
import { RecurringPicker } from '../ScheduleBuilder/RecurringPicker';
import { RecurringConfig } from '../../types';
import { useUiStore } from '../../store/uiStore';

const DEFAULT_SCHEDULE: RecurringConfig = {
  intervalUnit: 'hours',
  intervalValue: 1,
  timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  missedRunPolicy: 'skip' as const,
};

interface ProjectPulseEditorProps {
  agentId: string;
  agentName: string;
  projectId: string;
  open: boolean;
  onClose: () => void;
}

export function ProjectPulseEditor({
  agentId,
  agentName,
  projectId,
  open,
  onClose,
}: ProjectPulseEditorProps) {
  const queryClient = useQueryClient();
  const { openAgentChat } = useUiStore();

  const [content, setContent] = useState('');
  const [schedule, setSchedule] = useState<RecurringConfig>(DEFAULT_SCHEDULE);
  const [enabled, setEnabled] = useState(false);
  const [saving, setSaving] = useState(false);
  const [triggering, setTriggering] = useState(false);

  const { data: pulseConfig } = useQuery<PulseConfig>({
    queryKey: ['pulse-config', agentId, projectId],
    queryFn: () => pulseApi.getConfig(agentId, projectId),
    enabled: open,
  });

  useEffect(() => {
    if (pulseConfig) {
      setContent(pulseConfig.content);
      setEnabled(pulseConfig.enabled);
      setSchedule(pulseConfig.schedule ?? DEFAULT_SCHEDULE);
    }
  }, [pulseConfig]);

  if (!open) return null;

  async function handleSave() {
    setSaving(true);
    try {
      await pulseApi.update(agentId, projectId, content, schedule, enabled);
      queryClient.invalidateQueries({ queryKey: ['pulse-config', agentId, projectId] });
      queryClient.invalidateQueries({ queryKey: ['schedules'] });
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
      onClose();
    } catch (err) {
      console.error('Failed to save pulse:', err);
    }
    setSaving(false);
  }

  async function handleRunNow() {
    if (!pulseConfig?.taskId) return;
    setTriggering(true);
    try {
      await tasksApi.trigger(pulseConfig.taskId);
      queryClient.invalidateQueries({ queryKey: ['pulse-config', agentId, projectId] });
    } catch (err) {
      console.error('Failed to trigger pulse:', err);
    }
    setTriggering(false);
  }

  const canSave = content.trim().length > 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="w-[640px] max-h-[90vh] flex flex-col rounded-2xl border border-edge bg-panel shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-edge shrink-0">
          <div className="flex items-center gap-2 min-w-0">
            <Zap size={16} className="text-warning shrink-0" />
            <h3 className="text-base font-semibold text-white truncate">
              Pulse · {agentName}
            </h3>
          </div>
          <div className="flex items-center gap-3 shrink-0">
            <span className={`text-xs ${enabled ? 'text-emerald-400' : 'text-muted'}`}>
              {enabled ? 'Active' : 'Inactive'}
            </span>
            <Switch.Root
              checked={enabled}
              onCheckedChange={setEnabled}
              className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-emerald-500 transition-colors outline-none"
            >
              <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
            </Switch.Root>
            <button
              onClick={onClose}
              className="p-1.5 rounded text-muted hover:text-white hover:bg-edge"
            >
              <X size={16} />
            </button>
          </div>
        </div>

        {/* Body */}
        <div className="px-6 py-5 space-y-4 overflow-y-auto">
          <p className="text-xs text-muted">
            Define a recurring prompt that runs automatically on a schedule for this agent in this
            project. Responses are logged to a dedicated Pulse chat session.
          </p>

          <div>
            <label className="text-xs text-muted mb-1 block">Prompt</label>
            <Textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              rows={6}
              className="bg-background px-3 py-2 font-mono leading-relaxed"
              placeholder="Describe what this agent should do on each pulse..."
            />
          </div>

          <div>
            <label className="text-xs text-muted mb-1 block">Frequency</label>
            <RecurringPicker value={schedule} onChange={setSchedule} />
          </div>

          {pulseConfig?.nextRunAt && enabled && (
            <div className="flex items-center gap-2 text-xs text-muted">
              <Clock size={11} />
              <span>Next run: {new Date(pulseConfig.nextRunAt).toLocaleString()}</span>
            </div>
          )}
          {pulseConfig?.lastRunAt && (
            <div className="flex items-center gap-2 text-xs text-muted">
              <Clock size={11} />
              <span>Last run: {new Date(pulseConfig.lastRunAt).toLocaleString()}</span>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between gap-2 px-6 py-4 border-t border-edge shrink-0">
          <div className="flex items-center gap-2">
            {pulseConfig?.taskId && (
              <button
                onClick={handleRunNow}
                disabled={triggering}
                className="flex items-center gap-1.5 px-3 py-2 rounded-lg border border-edge text-secondary hover:text-white hover:border-edge-hover disabled:opacity-50 text-xs transition-colors"
              >
                <Play size={12} />
                {triggering ? 'Running...' : 'Run Now'}
              </button>
            )}
            {pulseConfig?.sessionId && (
              <button
                onClick={() => openAgentChat(agentId, pulseConfig.sessionId)}
                className="flex items-center gap-1.5 px-3 py-2 rounded-lg border border-edge text-secondary hover:text-white hover:border-edge-hover text-xs transition-colors"
              >
                <ExternalLink size={12} />
                View Pulse Log
              </button>
            )}
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={onClose}
              className="px-4 py-2 rounded-lg text-muted hover:text-white text-sm"
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              disabled={saving || !canSave}
              className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-sm font-medium transition-colors"
            >
              <Save size={14} />
              {saving ? 'Saving…' : 'Save'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
