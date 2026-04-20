import { openUrl } from '@tauri-apps/plugin-opener';
import type { ComponentProps } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { StreamingCursor } from './StreamingCursor';
import { MentionPill } from '../../features/mentions/MentionPill';

interface TextBlockProps {
  text: string;
  isStreaming: boolean;
}

function isExternalHref(href: string): boolean {
  try {
    const url = new URL(href, window.location.href);
    if (url.protocol === 'mailto:' || url.protocol === 'tel:') {
      return true;
    }
    return url.origin !== window.location.origin;
  } catch {
    return false;
  }
}

async function handleLinkClick(
  event: React.MouseEvent<HTMLAnchorElement>,
  href: string | undefined
) {
  if (!href || !isExternalHref(href)) {
    return;
  }

  event.preventDefault();

  try {
    await openUrl(href);
  } catch (error) {
    console.warn('Failed to open external link with system opener:', error);
    window.open(href, '_blank', 'noopener,noreferrer');
  }
}

export function TextBlock({ text, isStreaming }: TextBlockProps) {
  return (
    <div className="text-sm text-primary leading-relaxed chat-markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children, ...props }: ComponentProps<'a'>) => {
            if (typeof href === 'string' && href.startsWith('mention:')) {
              return <MentionPill href={href}>{children}</MentionPill>;
            }
            const external = typeof href === 'string' && isExternalHref(href);

            return (
              <a
                {...props}
                href={href}
                target={external ? '_blank' : props.target}
                rel={external ? 'noopener noreferrer' : props.rel}
                onClick={(event) => handleLinkClick(event, href)}
              >
                {children}
              </a>
            );
          },
        }}
      >
        {text}
      </ReactMarkdown>
      {isStreaming && (
        <div className="mt-2">
          <StreamingCursor />
        </div>
      )}
    </div>
  );
}
