import { Plugin } from '@orbit/plugin-sdk';
import WebSocket from 'ws';
import { randomUUID } from 'node:crypto';

const PLUGIN_ID = 'com.orbit.discord';
const GATEWAY_URL = 'wss://gateway.discord.gg/?v=10&encoding=json';
const API_BASE = 'https://discord.com/api/v10';
const TRIGGER_KIND = 'trigger.com_orbit_discord.message';

process.stderr.write(`discord plugin: boot (node=${process.version}, pid=${process.pid})\n`);
process.on('uncaughtException', (err) => {
  process.stderr.write(`discord plugin: uncaught ${err?.stack ?? err}\n`);
});
process.on('unhandledRejection', (err) => {
  process.stderr.write(`discord plugin: unhandled rejection ${err?.stack ?? err}\n`);
});

const plugin = new Plugin({ id: PLUGIN_ID });

const subscriptions = new Map();
let botToken = null;
let botUserId = null;
let gateway = null;

// Active typing-indicator pulse timers, keyed by `channelId:threadId`. Discord
// clears the indicator ~10s after the last POST, so we refresh on an interval.
const typingTimers = new Map();
const TYPING_PULSE_MS = 7000;
const TYPING_MAX_MS = 2 * 60 * 1000; // safety cap: 2 minutes even if stop is missed.

function normalizeOptionalId(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed || null;
}

function resolveTargetId(input) {
  const channelId = normalizeOptionalId(input?.channelId);
  const threadId = normalizeOptionalId(input?.threadId);
  if (!channelId) {
    throw new Error('channelId is required');
  }
  return { channelId, threadId, targetId: threadId ?? channelId };
}

// Discord channel types where the bot can post a message. Filters out
// categories (4) and directories (14); leaves text (0), announcement (5),
// voice text-chat (2, 13), threads (10-12), and forum (15) in place.
const MESSAGEABLE_CHANNEL_TYPES = new Set([0, 2, 5, 10, 11, 12, 13, 15]);

function isMessageableChannel(channel) {
  if (!channel || typeof channel !== 'object') return false;
  if (typeof channel.type !== 'number') return true;
  return MESSAGEABLE_CHANNEL_TYPES.has(channel.type);
}

function subscriptionKey(channelId, threadId) {
  return threadId ? `${channelId}:${threadId}` : channelId;
}

function matchesSubscription(channelId, threadId) {
  if (subscriptions.has(subscriptionKey(channelId, threadId))) return true;
  if (threadId && subscriptions.has(subscriptionKey(channelId, null))) return true;
  return false;
}

function getBotToken(_oauth) {
  // Discord's bot API requires a bot token from the Developer Portal, not an
  // OAuth access_token. The user pastes it into Plugins → Discord → Secrets.
  const token = process.env.ORBIT_DISCORD_BOT_TOKEN;
  if (!token) {
    throw new Error('Discord bot token not set (Plugins → Discord → Secrets → Bot Token)');
  }
  return token;
}

async function discordFetch(path, init = {}) {
  if (!botToken) throw new Error('bot token missing');
  const res = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: {
      Authorization: `Bot ${botToken}`,
      'Content-Type': 'application/json',
      'User-Agent': 'DiscordBot (orbit, 0.1.0)',
      ...(init.headers ?? {}),
    },
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Discord ${res.status}: ${text}`);
  }
  return res.status === 204 ? null : res.json();
}

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
          properties: {
            channelId: { type: 'string' },
            threadId: { type: 'string' },
          },
        },
      },
    },
  },
  run: async ({ input, oauth, log }) => {
    const list = Array.isArray(input.subscriptions) ? input.subscriptions : [];
    subscriptions.clear();
    for (const sub of list) {
      if (!sub?.channelId) continue;
      subscriptions.set(subscriptionKey(sub.channelId, sub.threadId ?? null), {
        channelId: sub.channelId,
        threadId: sub.threadId ?? null,
      });
    }
    log(`subscriptions set: ${subscriptions.size}`);
    try {
      botToken = getBotToken(oauth);
      ensureGateway();
    } catch (e) {
      log(`gateway not started: ${e.message}`);
    }
    return { count: subscriptions.size };
  },
});

plugin.tool('send_message', {
  description: 'Send a message as the bot.',
  inputSchema: {
    type: 'object',
    required: ['channelId', 'text'],
    properties: {
      channelId: { type: 'string' },
      threadId: { type: 'string' },
      text: { type: 'string' },
    },
  },
  run: async ({ input, oauth }) => {
    botToken = getBotToken(oauth);
    const { targetId } = resolveTargetId(input);
    const data = await discordFetch(`/channels/${targetId}/messages`, {
      method: 'POST',
      body: JSON.stringify({ content: input.text }),
    });
    return { messageId: data?.id ?? null };
  },
});

plugin.tool('add_reaction', {
  description: 'React to a Discord message. Supply a unicode emoji (e.g. "✅") or a custom emoji in the form "name:id".',
  inputSchema: {
    type: 'object',
    required: ['channelId', 'messageId', 'emoji'],
    properties: {
      channelId: { type: 'string' },
      threadId: { type: 'string' },
      messageId: { type: 'string' },
      emoji: { type: 'string' },
    },
  },
  run: async ({ input, oauth }) => {
    botToken = getBotToken(oauth);
    const { targetId } = resolveTargetId(input);
    const messageId = normalizeOptionalId(input?.messageId);
    const emoji = typeof input?.emoji === 'string' ? input.emoji.trim() : '';
    if (!messageId) throw new Error('messageId is required');
    if (!emoji) throw new Error('emoji is required');
    await discordFetch(
      `/channels/${targetId}/messages/${messageId}/reactions/${encodeURIComponent(emoji)}/@me`,
      { method: 'PUT' },
    );
    return { ok: true };
  },
});

plugin.tool('remove_reaction', {
  description: 'Remove the bot\'s own reaction from a Discord message.',
  inputSchema: {
    type: 'object',
    required: ['channelId', 'messageId', 'emoji'],
    properties: {
      channelId: { type: 'string' },
      threadId: { type: 'string' },
      messageId: { type: 'string' },
      emoji: { type: 'string' },
    },
  },
  run: async ({ input, oauth }) => {
    botToken = getBotToken(oauth);
    const { targetId } = resolveTargetId(input);
    const messageId = normalizeOptionalId(input?.messageId);
    const emoji = typeof input?.emoji === 'string' ? input.emoji.trim() : '';
    if (!messageId) throw new Error('messageId is required');
    if (!emoji) throw new Error('emoji is required');
    await discordFetch(
      `/channels/${targetId}/messages/${messageId}/reactions/${encodeURIComponent(emoji)}/@me`,
      { method: 'DELETE' },
    );
    return { ok: true };
  },
});

plugin.tool('start_typing', {
  description: 'Show the "Bot is typing…" indicator in a channel or thread. Idempotent — calling again extends the pulse. Auto-stops after 2 minutes as a safety cap.',
  inputSchema: {
    type: 'object',
    required: ['channelId'],
    properties: {
      channelId: { type: 'string' },
      threadId: { type: 'string' },
    },
  },
  run: async ({ input, oauth }) => {
    botToken = getBotToken(oauth);
    const { channelId, threadId, targetId } = resolveTargetId(input);
    const key = subscriptionKey(channelId, threadId);
    const existing = typingTimers.get(key);
    if (existing) {
      clearInterval(existing.interval);
      clearTimeout(existing.cap);
    }
    const pulse = () => {
      discordFetch(`/channels/${targetId}/typing`, { method: 'POST' }).catch((err) => {
        process.stderr.write(`typing pulse failed: ${err.message}\n`);
      });
    };
    pulse();
    const interval = setInterval(pulse, TYPING_PULSE_MS);
    const cap = setTimeout(() => {
      clearInterval(interval);
      typingTimers.delete(key);
    }, TYPING_MAX_MS);
    typingTimers.set(key, { interval, cap });
    return { ok: true };
  },
});

plugin.tool('stop_typing', {
  description: 'Stop the typing indicator for a channel or thread. Discord still shows the last indicator for ~10s after the final POST.',
  inputSchema: {
    type: 'object',
    required: ['channelId'],
    properties: {
      channelId: { type: 'string' },
      threadId: { type: 'string' },
    },
  },
  run: async ({ input }) => {
    const { channelId, threadId } = resolveTargetId(input);
    const key = subscriptionKey(channelId, threadId);
    const existing = typingTimers.get(key);
    if (existing) {
      clearInterval(existing.interval);
      clearTimeout(existing.cap);
      typingTimers.delete(key);
    }
    return { ok: true };
  },
});

plugin.tool('list_channels', {
  description: 'List channels the bot can see.',
  inputSchema: {
    type: 'object',
    properties: { guildId: { type: 'string' } },
  },
  run: async ({ input, oauth }) => {
    botToken = getBotToken(oauth);
    if (input.guildId) {
      const channels = await discordFetch(`/guilds/${input.guildId}/channels`);
      return { channels: channels.filter(isMessageableChannel) };
    }
    // No guildId: return every guild the bot is in, each with its channels
    // embedded. Callers that want the full list (workflow inspector) get it in
    // one call; callers that drill down (agent listen-channel picker) still
    // find `guilds[].channels` populated without a second round-trip.
    const guilds = await discordFetch('/users/@me/guilds');
    const enriched = await Promise.all(
      (guilds ?? []).map(async (guild) => {
        try {
          const channels = await discordFetch(`/guilds/${guild.id}/channels`);
          return { ...guild, channels: channels.filter(isMessageableChannel) };
        } catch (err) {
          process.stderr.write(
            `discord list_channels: guild ${guild.id} channels failed: ${err?.message ?? err}\n`,
          );
          return { ...guild, channels: [] };
        }
      }),
    );
    return { guilds: enriched };
  },
});

function ensureGateway() {
  if (gateway) return;
  if (!botToken) return;
  connectGateway();
}

function connectGateway() {
  const ws = new WebSocket(GATEWAY_URL);
  gateway = ws;
  let heartbeatTimer = null;
  let lastSeq = null;

  const send = (op, d) => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ op, d }));
    }
  };

  ws.on('open', () => {
    process.stderr.write('discord gateway: open\n');
  });

  ws.on('message', async (raw) => {
    let msg;
    try {
      msg = JSON.parse(raw.toString('utf8'));
    } catch {
      return;
    }
    if (msg.s != null) lastSeq = msg.s;

    switch (msg.op) {
      case 10: {
        const interval = msg.d?.heartbeat_interval ?? 41250;
        heartbeatTimer = setInterval(() => send(1, lastSeq), interval);
        send(2, {
          token: botToken,
          intents: (1 << 0) | (1 << 9) | (1 << 15),
          properties: {
            os: process.platform,
            browser: 'orbit',
            device: 'orbit',
          },
        });
        break;
      }
      case 0: {
        if (msg.t === 'READY') {
          botUserId = msg.d?.user?.id ?? null;
          process.stderr.write(`discord gateway: ready (${botUserId})\n`);
        } else if (msg.t === 'MESSAGE_CREATE') {
          await handleMessageCreate(msg.d);
        }
        break;
      }
      case 7:
      case 9: {
        process.stderr.write(`discord gateway: op=${msg.op}, reconnecting\n`);
        ws.close();
        break;
      }
      default:
        break;
    }
  });

  ws.on('close', (code) => {
    if (heartbeatTimer) clearInterval(heartbeatTimer);
    heartbeatTimer = null;
    gateway = null;
    process.stderr.write(`discord gateway: closed (${code}); reconnect in 3s\n`);
    setTimeout(() => {
      if (botToken && subscriptions.size > 0) connectGateway();
    }, 3000);
  });

  ws.on('error', (err) => {
    process.stderr.write(`discord gateway: error ${err.message}\n`);
  });
}

async function handleMessageCreate(d) {
  if (!d) return;
  const channelId = d.channel_id;
  if (!channelId) return;
  const isThreadMessage = !!(d.thread ?? d.message_reference?.channel_id);
  const threadId = isThreadMessage ? channelId : null;
  const effectiveChannelId = threadId ? d.thread?.parent_id ?? channelId : channelId;

  if (!matchesSubscription(effectiveChannelId, threadId)) return;
  if (d.author?.bot) return;

  const mentions = Array.isArray(d.mentions) ? d.mentions.map((m) => m?.id).filter(Boolean) : [];
  const payload = {
    eventId: d.id ?? randomUUID(),
    pluginId: PLUGIN_ID,
    kind: TRIGGER_KIND,
    channel: {
      id: effectiveChannelId,
      threadId: threadId ?? undefined,
      name: d.channel_name ?? undefined,
      workspaceId: d.guild_id ?? undefined,
    },
    user: {
      id: d.author?.id ?? 'unknown',
      displayName: d.author?.global_name ?? d.author?.username ?? undefined,
      bot: !!d.author?.bot,
    },
    text: d.content ?? '',
    mentions: botUserId && mentions.includes(botUserId) ? [botUserId] : [],
    receivedAt: new Date().toISOString(),
    raw: d,
  };

  try {
    await plugin.core.triggers.emit(payload);
  } catch (e) {
    process.stderr.write(`trigger.emit failed: ${e.message}\n`);
  }
}

plugin.run();
