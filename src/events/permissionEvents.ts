import { listen } from '@tauri-apps/api/event';
import { PermissionRequestPayload, PermissionCancelledPayload } from '../types';

export function onPermissionRequest(handler: (payload: PermissionRequestPayload) => void) {
  return listen<PermissionRequestPayload>('permission:request', (event) => {
    handler(event.payload);
  });
}

export function onPermissionCancelled(handler: (payload: PermissionCancelledPayload) => void) {
  return listen<PermissionCancelledPayload>('permission:cancelled', (event) => {
    handler(event.payload);
  });
}
