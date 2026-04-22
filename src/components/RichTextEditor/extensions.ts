import StarterKit from '@tiptap/starter-kit';
import Link from '@tiptap/extension-link';
import Placeholder from '@tiptap/extension-placeholder';
import type { Extensions } from '@tiptap/core';

export function baseExtensions(placeholder?: string): Extensions {
  return [
    StarterKit.configure({
      heading: { levels: [1, 2, 3] },
    }),
    Link.configure({
      openOnClick: false,
      autolink: true,
      linkOnPaste: true,
      HTMLAttributes: {
        class: 'text-accent underline underline-offset-2 hover:text-accent-hover',
        rel: 'noopener noreferrer',
      },
    }),
    Placeholder.configure({ placeholder: placeholder ?? '' }),
  ];
}
