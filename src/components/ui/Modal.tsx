import * as Dialog from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import { type ReactNode } from 'react';
import { cn } from '../../lib/cn';

export type ModalSize = 'sm' | 'md' | 'lg' | 'xl';

const sizeClass: Record<ModalSize, string> = {
  sm: 'w-[400px]',
  md: 'w-[560px]',
  lg: 'w-[800px]',
  xl: 'w-[1080px] max-w-[94vw]',
};

interface ModalProps {
  open: boolean;
  onClose: () => void;
  size?: ModalSize;
  children: ReactNode;
  className?: string;
  /** Pass a label for the Dialog title when the header isn't a plain string. */
  ariaTitle?: string;
  /** If true, clicks on the overlay won't close the modal. */
  disableOutsideClose?: boolean;
  /** Called when Escape or outside-click is attempted; return false to cancel. */
  onBeforeClose?: () => boolean | Promise<boolean>;
}

export function Modal({
  open,
  onClose,
  size = 'lg',
  children,
  className,
  ariaTitle,
  disableOutsideClose = false,
  onBeforeClose,
}: ModalProps) {
  const handleOpenChange = async (next: boolean) => {
    if (next) return;
    if (onBeforeClose) {
      const ok = await onBeforeClose();
      if (!ok) return;
    }
    onClose();
  };

  return (
    <Dialog.Root open={open} onOpenChange={handleOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/60 data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=closed]:animate-out data-[state=closed]:fade-out-0" />
        <Dialog.Content
          onInteractOutside={(e) => {
            if (disableOutsideClose) e.preventDefault();
          }}
          className={cn(
            'fixed left-1/2 top-1/2 z-50 -translate-x-1/2 -translate-y-1/2',
            'max-h-[92vh] overflow-hidden rounded-2xl border border-edge bg-panel shadow-2xl outline-none',
            'flex flex-col',
            sizeClass[size],
            className,
          )}
        >
          {ariaTitle && (
            <Dialog.Title className="sr-only">{ariaTitle}</Dialog.Title>
          )}
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface ModalCloseButtonProps {
  onClick: () => void;
  className?: string;
}

export function ModalCloseButton({ onClick, className }: ModalCloseButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'p-1.5 rounded text-muted hover:text-white hover:bg-edge transition-colors',
        className,
      )}
      aria-label="Close"
    >
      <X size={16} />
    </button>
  );
}
