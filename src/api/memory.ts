import { invoke } from '@tauri-apps/api/core';
import { MemoryEntry, MemoryType } from '../types';

export const memoryApi = {
  search: (
    query: string,
    memoryType?: MemoryType,
    limit?: number,
  ): Promise<MemoryEntry[]> =>
    invoke('search_memories', { query, memoryType, limit }),

  list: (
    memoryType?: MemoryType,
    limit?: number,
    offset?: number,
  ): Promise<MemoryEntry[]> =>
    invoke('list_memories', { memoryType, limit, offset }),

  add: (text: string, memoryType: MemoryType): Promise<MemoryEntry[]> =>
    invoke('add_memory', { text, memoryType }),

  delete: (memoryId: string): Promise<void> => invoke('delete_memory', { memoryId }),

  update: (memoryId: string, text: string): Promise<MemoryEntry> =>
    invoke('update_memory', { memoryId, text }),

  getHealth: (): Promise<boolean> => invoke('get_memory_health'),
};
