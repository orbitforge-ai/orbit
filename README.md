# Orbit

A macOS desktop automation and AI agent orchestration platform. Orbit lets you create, schedule, and execute automated tasks and AI agent workflows with a real-time chat interface, multi-agent coordination, and run history tracking.

## Features

- **Task orchestration** — shell commands, HTTP requests, and AI agent loops
- **AI agent chat** — conversational interface backed by Claude (Anthropic) or MiniMax
- **Multi-agent coordination** — agents communicate via a shared message bus
- **Scheduling** — cron-based and on-demand task execution
- **Permissions system** — fine-grained control over what agents can do
- **Memory integration** — persistent agent memory via Mem0
- **Cloud sync** — authentication and data sync via Supabase
- **Run history** — full execution logs and status tracking

## Tech Stack

| Layer | Technology |
| --- | --- |
| Frontend | React 19, TypeScript, Vite, Tailwind CSS |
| Desktop shell | Tauri 2 (Rust) |
| State | Zustand, TanStack Query |
| Local DB | SQLite (via rusqlite) |
| Cloud | Supabase (auth + PostgreSQL) |
| LLM providers | Anthropic Claude, MiniMax |
| Memory | Mem0 |

## Prerequisites

- **macOS** 12.0 or later
- **Node.js** 22+ and **pnpm** (`npm install -g pnpm`)
- **Rust** 1.80+ — install via [rustup](https://rustup.rs)
- **Tauri CLI** prerequisites — see [Tauri v2 setup guide](https://v2.tauri.app/start/prerequisites/)

## Setup

### 1. Clone and install dependencies

```bash
git clone <repo-url>
cd orbit
pnpm install
```

### 2. Configure environment variables

Create `src-tauri/.env` with the following:

```env
SUPABASE_URL=https://your-project.supabase.co
SUPABASE_ANON_KEY=your-anon-key
SUPABASE_PASSWORD=your-db-password
MEM0_API_KEY=your-mem0-api-key
```

- `SUPABASE_URL` / `SUPABASE_ANON_KEY` / `SUPABASE_PASSWORD` — from your [Supabase project settings](https://supabase.com/dashboard)
- `MEM0_API_KEY` — from [Mem0](https://mem0.ai); optional, memory features are disabled if not set

These variables are embedded into the binary at build time via `src-tauri/build.rs`.

### 3. Run in development

```bash
pnpm tauri dev
```

This starts the Vite dev server and the Tauri desktop window together with hot reloading.

## Commands

| Command | Description |
| --- | --- |
| `pnpm tauri dev` | Start the app in development mode |
| `pnpm tauri build` | Build a production `.app` bundle |
| `pnpm dev` | Start only the Vite frontend (no desktop window) |
| `pnpm build` | Build the frontend only |
| `pnpm preview` | Preview the built frontend |
| `pnpm format` | Format all files with Prettier |
| `pnpm format:check` | Check formatting without writing |

## Project Structure

```text
orbit/
├── src/                    # React frontend
│   ├── screens/            # Top-level pages (Dashboard, Chat, Tasks, etc.)
│   ├── components/         # Reusable UI components
│   ├── api/                # Tauri command bindings
│   ├── store/              # Zustand state stores
│   └── types/              # Shared TypeScript types
│
├── src-tauri/              # Rust/Tauri backend
│   ├── src/
│   │   ├── commands/       # Tauri IPC command handlers
│   │   ├── executor/       # Task and agent execution engine
│   │   ├── scheduler/      # Cron-based scheduler
│   │   ├── models/         # Data structures
│   │   ├── db/             # SQLite + Supabase integration
│   │   └── lib.rs          # App entry point
│   ├── tauri.conf.json     # App configuration
│   └── .env                # Environment variables (not committed)
│
├── supabase/
│   └── schema.sql          # Database schema
│
└── public/                 # Static frontend assets
```

## Data Directory

Orbit stores runtime data at `~/.orbit/`:

```text
~/.orbit/
├── logs/       # Application logs
└── skills/     # Custom agent skills
```

## IDE Setup

- [VS Code](https://code.visualstudio.com/) with the [Tauri extension](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) and [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
