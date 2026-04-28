import { create } from 'zustand';

interface TerminalStore {
  open: boolean;
  title: string;
  toggle: () => void;
  setOpen: (open: boolean) => void;
  setTitle: (title: string) => void;
}

export const useTerminalStore = create<TerminalStore>((set) => ({
  open: false,
  title: '',
  toggle: () => set((s) => ({ open: !s.open })),
  setOpen: (open) => set({ open }),
  setTitle: (title) => set({ title }),
}));
