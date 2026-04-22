import {
  AlertOctagon,
  ArrowRight,
  CheckCircle,
  FilePlus,
  Flag,
  MessageSquare,
  MessageSquarePlus,
  MessageSquareX,
  PenLine,
  Tag,
  UserCog,
} from 'lucide-react';
import type { WorkItemEvent, WorkItemEventKind } from '../../../types';

interface Rendered {
  Icon: React.ComponentType<{ size?: number; className?: string }>;
  iconClass: string;
  title: React.ReactNode;
  subline?: React.ReactNode;
}

const PRIORITY_LABEL = ['Low', 'Medium', 'High', 'Urgent'];

function payloadStr(payload: Record<string, unknown>, key: string): string | null {
  const v = payload[key];
  return typeof v === 'string' ? v : null;
}

function payloadNum(payload: Record<string, unknown>, key: string): number | null {
  const v = payload[key];
  return typeof v === 'number' ? v : null;
}

export function renderEvent(event: WorkItemEvent): Rendered {
  const kind = event.kind as WorkItemEventKind;
  const p = (event.payload ?? {}) as Record<string, unknown>;

  switch (kind) {
    case 'created':
      return {
        Icon: FilePlus,
        iconClass: 'text-accent-light',
        title: <span>created this item</span>,
      };

    case 'title_changed': {
      const to = payloadStr(p, 'to');
      return {
        Icon: PenLine,
        iconClass: 'text-secondary',
        title: <span>renamed to <span className="text-white">"{to ?? '…'}"</span></span>,
      };
    }

    case 'description_changed':
      return {
        Icon: PenLine,
        iconClass: 'text-secondary',
        title: <span>edited the description</span>,
      };

    case 'kind_changed': {
      const from = payloadStr(p, 'from');
      const to = payloadStr(p, 'to');
      return {
        Icon: Tag,
        iconClass: 'text-secondary',
        title: (
          <span>
            changed kind{' '}
            <span className="text-muted">{from}</span> → <span className="text-white">{to}</span>
          </span>
        ),
      };
    }

    case 'priority_changed': {
      const from = payloadNum(p, 'from');
      const to = payloadNum(p, 'to');
      return {
        Icon: Flag,
        iconClass: 'text-orange-400',
        title: (
          <span>
            priority{' '}
            <span className="text-muted">{from !== null ? PRIORITY_LABEL[from] : '…'}</span> →{' '}
            <span className="text-white">{to !== null ? PRIORITY_LABEL[to] : '…'}</span>
          </span>
        ),
      };
    }

    case 'labels_changed': {
      const added = Array.isArray(p.added) ? (p.added as string[]) : [];
      const removed = Array.isArray(p.removed) ? (p.removed as string[]) : [];
      const parts: React.ReactNode[] = [];
      if (added.length) parts.push(<span key="a">added <span className="text-white">{added.join(', ')}</span></span>);
      if (removed.length)
        parts.push(
          <span key="r">
            {parts.length ? '; ' : ''}removed <span className="text-white">{removed.join(', ')}</span>
          </span>,
        );
      return {
        Icon: Tag,
        iconClass: 'text-accent-light',
        title: parts.length ? <span>{parts}</span> : <span>updated labels</span>,
      };
    }

    case 'column_changed': {
      const fromName = payloadStr(p, 'fromName') ?? payloadStr(p, 'from');
      const toName = payloadStr(p, 'toName') ?? payloadStr(p, 'to');
      return {
        Icon: ArrowRight,
        iconClass: 'text-accent-light',
        title: (
          <span>
            moved{' '}
            <span className="text-muted">{fromName ?? '—'}</span> →{' '}
            <span className="text-white">{toName ?? '—'}</span>
          </span>
        ),
      };
    }

    case 'assignee_changed': {
      const toName = payloadStr(p, 'toName') ?? payloadStr(p, 'to');
      return {
        Icon: UserCog,
        iconClass: 'text-accent-light',
        title: toName ? (
          <span>
            assigned to <span className="text-white">{toName}</span>
          </span>
        ) : (
          <span>changed assignee</span>
        ),
      };
    }

    case 'blocked': {
      const reason = payloadStr(p, 'reason');
      return {
        Icon: AlertOctagon,
        iconClass: 'text-red-400',
        title: <span>marked as blocked</span>,
        subline: reason ? <span className="text-red-200">{reason}</span> : undefined,
      };
    }

    case 'unblocked':
      return {
        Icon: AlertOctagon,
        iconClass: 'text-emerald-400',
        title: <span>unblocked</span>,
      };

    case 'completed':
      return {
        Icon: CheckCircle,
        iconClass: 'text-emerald-400',
        title: <span>marked as done</span>,
      };

    case 'comment_added':
      return {
        Icon: MessageSquarePlus,
        iconClass: 'text-accent-light',
        title: <span>added a comment</span>,
      };

    case 'comment_edited':
      return {
        Icon: MessageSquare,
        iconClass: 'text-secondary',
        title: <span>edited a comment</span>,
      };

    case 'comment_deleted':
      return {
        Icon: MessageSquareX,
        iconClass: 'text-muted',
        title: <span>deleted a comment</span>,
      };

    default:
      return {
        Icon: PenLine,
        iconClass: 'text-muted',
        title: <span>{String(kind)}</span>,
      };
  }
}
