import { Plugin } from '@orbit/plugin-sdk';
import WebSocket from 'ws';
import { randomUUID } from 'node:crypto';

const PLUGIN_ID = 'com.orbit.slack';
const API_BASE = 'https://slack.com/api';
const TRIGGER_KIND = 'trigger.com_orbit_slack.message';

process.stderr.write(`slack plugin: boot (node=${process.version}, pid=${process.pid})\n`);
process.on('uncaughtException', (err) => {
  process.stderr.write(`slack plugin: uncaught ${err?.stack ?? err}\n`);
});
process.on('unhandledRejection', (err) => {
  process.stderr.write(`slack plugin: unhandled rejection ${err?.stack ?? err}\n`);
});

const plugin = new Plugin({ id: PLUGIN_ID });

// channel IDs the agent wants to receive messages from
const subscriptions = new Set();

let botToken = null;
let appToken = null;
let botUserId = null;
let socketMode = null; // active WebSocket connection

function getBotToken() {
  const token = process.env.ORBIT_SLACK_BOT_TOKEN;
  if (!token) {
    throw new Error('Slack bot token not set (Plugins → Slack → Secrets → Bot Token)');
  }
  return token;
}

function getAppToken() {
  const token = process.env.ORBIT_SLACK_APP_TOKEN;
  if (!token) {
    throw new Error('Slack app-level token not set (Plugins → Slack → Secrets → App-Level Token)');
  }
  return token;
}

async function slackFetch(method, params = {}, token = null) {
  const tok = token ?? botToken;
  if (!tok) throw new Error('bot token missing');

  const res = await fetch(`${API_BASE}/${method}`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${tok}`,
      'Content-Type': 'application/json; charset=utf-8',
    },
    body: JSON.stringify(params),
  });

  const data = await res.json();
  if (!data.ok) {
    throw new Error(`Slack API error (${method}): ${data.error ?? 'unknown'}`);
  }
  return data;
}

// ── Socket Mode ────────────────────────────────────────────────────────────────

async function openSocketModeConnection() {
  const data = await slackFetch('apps.connections.open', {}, appToken);
  const wsUrl = data.url;
  if (!wsUrl) throw new Error('apps.connections.open did not return a url');
  connectSocketMode(wsUrl);
}

function connectSocketMode(wsUrl) {
  if (socketMode) {
    try { socketMode.terminate(); } catch {}
    socketMode = null;
  }

  const ws = new WebSocket(wsUrl);
  socketMode = ws;

  ws.on('open', () => {
    process.stderr.write('slack socket mode: open\n');
  });

  ws.on('message', async (raw) => {
    let msg;
    try {
      msg = JSON.parse(raw.toString('utf8'));
    } catch {
      return;
    }

    switch (msg.type) {
      case 'hello':
        process.stderr.write('slack socket mode: hello (ready)\n');
        break;

      case 'events_api': {
        // ACK immediately to prevent retry
        if (msg.envelope_id) {
          ws.send(JSON.stringify({ envelope_id: msg.envelope_id }));
        }
        const event = msg.payload?.event;
        if (event?.type === 'message') {
          await handleMessageEvent(event).catch((err) => {
            process.stderr.write(`slack handle message failed: ${err?.message ?? err}\n`);
          });
        }
        break;
      }

      case 'disconnect': {
        process.stderr.write(`slack socket mode: disconnect (${msg.reason ?? 'no reason'})\n`);
        ws.close();
        break;
      }

      default:
        break;
    }
  });

  ws.on('close', (code) => {
    socketMode = null;
    process.stderr.write(`slack socket mode: closed (${code}); reconnect in 5s\n`);
    setTimeout(() => {
      if (subscriptions.size > 0 && botToken && appToken) {
        openSocketModeConnection().catch((err) => {
          process.stderr.write(`slack socket mode: reconnect failed: ${err?.message ?? err}\n`);
        });
      }
    }, 5000);
  });

  ws.on('error', (err) => {
    process.stderr.write(`slack socket mode: error ${err.message}\n`);
  });
}

async function handleMessageEvent(event) {
  // Ignore bot messages and subtypes (edits, joins, etc.)
  if (event.subtype) return;
  if (event.bot_id) return;

  const channelId = event.channel;
  if (!channelId || !subscriptions.has(channelId)) return;

  // Resolve bot user id lazily so we can detect self-mentions
  if (!botUserId) {
    try {
      const auth = await slackFetch('auth.test', {});
      botUserId = auth.user_id ?? null;
    } catch {}
  }

  const mentions = [];
  if (botUserId && typeof event.text === 'string' && event.text.includes(`<@${botUserId}>`)) {
    mentions.push(botUserId);
  }

  const payload = {
    eventId: event.client_msg_id ?? event.ts ?? randomUUID(),
    pluginId: PLUGIN_ID,
    kind: TRIGGER_KIND,
    channel: {
      id: channelId,
      threadTs: event.thread_ts ?? undefined,
    },
    user: {
      id: event.user ?? 'unknown',
      bot: false,
    },
    text: event.text ?? '',
    mentions,
    receivedAt: new Date().toISOString(),
    raw: event,
  };

  try {
    await plugin.core.triggers.emit(payload);
  } catch (e) {
    process.stderr.write(`trigger.emit failed: ${e.message}\n`);
  }
}

function ensureSocketMode() {
  if (socketMode) return;
  if (!botToken || !appToken) return;
  openSocketModeConnection().catch((err) => {
    process.stderr.write(`slack socket mode: initial connect failed: ${err?.message ?? err}\n`);
  });
}

// ── Tools ──────────────────────────────────────────────────────────────────────

plugin.tool('set_subscriptions', {
  description: 'Replace the subscription set.',
  inputSchema: {
    type: 'object',
    required: ['subscriptions'],
    properties: {
      subscriptions: {
        type: 'array',
        items: {
          type: 'object',
          required: ['channelId'],
          properties: { channelId: { type: 'string' } },
        },
      },
    },
  },
  run: async ({ input, log }) => {
    const list = Array.isArray(input.subscriptions) ? input.subscriptions : [];
    subscriptions.clear();
    for (const sub of list) {
      if (sub?.channelId) subscriptions.add(sub.channelId.trim());
    }
    log(`subscriptions set: ${subscriptions.size}`);

    try {
      botToken = getBotToken();
      appToken = getAppToken();
      if (subscriptions.size > 0) {
        ensureSocketMode();
      } else if (socketMode) {
        socketMode.terminate();
        socketMode = null;
      }
    } catch (e) {
      log(`socket mode not started: ${e.message}`);
    }

    return { count: subscriptions.size };
  },
});

plugin.tool('list_channels', {
  description: 'List channels the bot can access.',
  inputSchema: {
    type: 'object',
    properties: {
      cursor: { type: 'string' },
      limit: { type: 'integer' },
    },
  },
  run: async ({ input }) => {
    botToken = getBotToken();
    const params = {
      types: 'public_channel,private_channel',
      exclude_archived: true,
      limit: Math.min(input.limit ?? 200, 1000),
    };
    if (input.cursor) params.cursor = input.cursor;

    const data = await slackFetch('conversations.list', params);
    return {
      channels: (data.channels ?? []).map((c) => ({
        id: c.id,
        name: c.name,
        isPrivate: c.is_private ?? false,
        isMember: c.is_member ?? false,
        topic: c.topic?.value ?? '',
      })),
      nextCursor: data.response_metadata?.next_cursor ?? null,
    };
  },
});

plugin.tool('send_message', {
  description: 'Post a message as the bot.',
  inputSchema: {
    type: 'object',
    required: ['channelId', 'text'],
    properties: {
      channelId: { type: 'string' },
      text: { type: 'string' },
      threadTs: { type: 'string' },
    },
  },
  run: async ({ input }) => {
    botToken = getBotToken();
    const params = { channel: input.channelId, text: input.text };
    if (input.threadTs) params.thread_ts = input.threadTs;
    const data = await slackFetch('chat.postMessage', params);
    return { ts: data.ts ?? null, channel: data.channel ?? null };
  },
});

plugin.tool('send_ephemeral', {
  description: 'Send an ephemeral message visible only to a specific user.',
  inputSchema: {
    type: 'object',
    required: ['channelId', 'userId', 'text'],
    properties: {
      channelId: { type: 'string' },
      userId: { type: 'string' },
      text: { type: 'string' },
    },
  },
  run: async ({ input }) => {
    botToken = getBotToken();
    const data = await slackFetch('chat.postEphemeral', {
      channel: input.channelId,
      user: input.userId,
      text: input.text,
    });
    return { messageTs: data.message_ts ?? null };
  },
});

plugin.tool('add_reaction', {
  description: 'React to a Slack message with an emoji name.',
  inputSchema: {
    type: 'object',
    required: ['channelId', 'timestamp', 'emoji'],
    properties: {
      channelId: { type: 'string' },
      timestamp: { type: 'string' },
      emoji: { type: 'string' },
    },
  },
  run: async ({ input }) => {
    botToken = getBotToken();
    const emoji = input.emoji.replace(/^:|:$/g, ''); // strip colons if provided
    await slackFetch('reactions.add', {
      channel: input.channelId,
      timestamp: input.timestamp,
      name: emoji,
    });
    return { ok: true };
  },
});

plugin.tool('list_messages', {
  description: 'Fetch recent messages from a channel.',
  inputSchema: {
    type: 'object',
    required: ['channelId'],
    properties: {
      channelId: { type: 'string' },
      limit: { type: 'integer' },
      oldest: { type: 'string' },
      latest: { type: 'string' },
    },
  },
  run: async ({ input }) => {
    botToken = getBotToken();
    const params = {
      channel: input.channelId,
      limit: Math.min(input.limit ?? 20, 100),
    };
    if (input.oldest) params.oldest = input.oldest;
    if (input.latest) params.latest = input.latest;

    const data = await slackFetch('conversations.history', params);
    return {
      messages: (data.messages ?? []).map((m) => ({
        ts: m.ts,
        user: m.user ?? m.bot_id ?? null,
        text: m.text ?? '',
        threadTs: m.thread_ts ?? null,
        replyCount: m.reply_count ?? 0,
      })),
      hasMore: data.has_more ?? false,
    };
  },
});

plugin.run();
