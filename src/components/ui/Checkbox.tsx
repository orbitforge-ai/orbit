import { forwardRef, type ReactNode } from 'react';
import * as RadixCheckbox from '@radix-ui/react-checkbox';
import { Check } from 'lucide-react';
import { cn } from '../../lib/cn';

export interface CheckboxProps
  extends Omit<React.ComponentPropsWithoutRef<typeof RadixCheckbox.Root>, 'children'> {
  label?: ReactNode;
  labelClassName?: string;
}

export const Checkbox = forwardRef<React.ElementRef<typeof RadixCheckbox.Root>, CheckboxProps>(
  function Checkbox({ className, label, labelClassName, id, ...rest }, ref) {
    const root = (
      <RadixCheckbox.Root
        ref={ref}
        id={id}
        className={cn(
          'flex h-4 w-4 items-center justify-center rounded border border-edge bg-surface text-white transition-colors focus:outline-none focus:ring-1 focus:ring-accent data-[state=checked]:bg-accent data-[state=checked]:border-accent disabled:opacity-50 disabled:cursor-not-allowed',
          className,
        )}
        {...rest}
      >
        <RadixCheckbox.Indicator>
          <Check size={12} strokeWidth={3} />
        </RadixCheckbox.Indicator>
      </RadixCheckbox.Root>
    );

    if (label === undefined) return root;
    return (
      <label
        htmlFor={id}
        className={cn(
          'inline-flex items-center gap-2 text-sm text-white cursor-pointer select-none',
          labelClassName,
        )}
      >
        {root}
        <span>{label}</span>
      </label>
    );
  },
);
