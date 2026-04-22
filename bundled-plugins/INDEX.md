# Orbit Bundled Plugins Index

This file tracks all bundled plugins in the Orbit project to avoid duplicative work.

---

## com.orbit.github
- **Version:** 0.1.2
- **Type:** MCP stdio (node server.js)
- **Description:** GitHub integration - clone repos, open PRs, git operations
- **Tools:**
  - `resolve_surface_actions` - sidebar actions for repos
  - `clone_repo` - clone GitHub repos
  - `git_pull` - run git pull --ff-only
  - `git_push` - push changes
  - `git_checkout_branch` - branch checkout
  - `create_pr` - open pull requests
- **OAuth:** GitHub (repo, read:org scopes)
- **Network Permissions:** api.github.com, github.com
- **UI:** surfaceActions for mainSidebar, workspaceBrowser

---

## com.orbit.discord
- **Version:** 0.1.2
- **Type:** MCP stdio (node server.js)
- **Description:** Discord bot integration - messaging, reactions, channel subscriptions
- **Tools:**
  - `set_subscriptions` - manage channel/thread subscriptions
  - `send_message` - send messages as bot
  - `add_reaction` - react to messages
  - `remove_reaction` - remove bot's reactions
  - `start_typing` / `stop_typing` - typing indicators
  - `list_channels` - list accessible channels
- **OAuth:** Discord (bot, applications.commands scopes)
- **Secrets:** botToken
- **Network Permissions:** discord.com, gateway.discord.gg, cdn.discordapp.com
- **Workflow:** trigger.com_orbit_discord.message, integration.com_orbit_discord.send_message

---

## com.orbit.slack
- **Version:** 0.1.0
- **Type:** MCP stdio (node server.js)
- **Description:** Slack bot integration - Socket Mode gateway, bidirectional messaging, channel subscriptions
- **Tools:**
  - `set_subscriptions` - manage channel subscriptions (starts/stops Socket Mode)
  - `list_channels` - list accessible public/private channels
  - `send_message` - post messages as bot (supports thread replies)
  - `send_ephemeral` - send user-only visible messages
  - `add_reaction` - react to messages with emoji
  - `list_messages` - fetch recent channel history
- **Secrets:** botToken (xoxb-...), appToken (xapp-... for Socket Mode)
- **Network Permissions:** slack.com, api.slack.com, wss-primary.slack.com, wss-backup.slack.com
- **Workflow:** trigger.com_orbit_slack.message, integration.com_orbit_slack.send_message

---

## Summary of Integrations
| Plugin | Category | Key Features |
|--------|----------|--------------|
| GitHub | Version Control | Clone, PRs, git operations |
| Discord | Communication | Messaging, reactions, subscriptions |
| Slack | Communication | Messaging, ephemeral, reactions, Socket Mode |

---

*Last Updated: 2026-04-21*
