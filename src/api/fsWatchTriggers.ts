import { invoke } from './transport';
import type { FsWatchEventType } from '../types';

export async function emitFsTriggerEvent(args: {
  workflowId: string;
  path: string;
  eventType: FsWatchEventType;
  isDir: boolean;
  watchedPath: string;
}): Promise<void> {
  return invoke<void>('emit_fs_trigger_event', args);
}
