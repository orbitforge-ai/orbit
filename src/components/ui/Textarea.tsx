import { forwardRef, type TextareaHTMLAttributes } from 'react';
import { cn } from '../../lib/cn';
import { inputCls } from '../../lib/taskConstants';

export type TextareaProps = TextareaHTMLAttributes<HTMLTextAreaElement>;

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(function Textarea(
  { className, ...rest },
  ref,
) {
  return (
    <textarea ref={ref} className={cn(inputCls, 'min-h-[80px] resize-y', className)} {...rest} />
  );
});
