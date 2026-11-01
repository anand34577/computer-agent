# Computer Agent

A desktop app where you chat with any AI model and, when the model is capable, let it **see and operate your computer**: clicking, typing, opening apps, managing files, running commands, and finishing multi-step jobs.

- **Safe by default:** risky steps ask first, fixed safety gates always hold, and one key stops everything.
- **Bring your own model:** cloud APIs, OpenAI-compatible services or local runtimes.
- **Extensible:** add plugins and MCP servers.
- **Honest:** each model's real capabilities are tested and shown.

Built from *Universal AI Computer-Use Agent: Complete Product Requirements (v2)*.

## Run it

You need Node 20+, Rust (stable, with MSVC on Windows) and WebView2, which Windows 11 already includes.

```bash
npm install
npm run tauri dev                      # development
npx tauri build --no-bundle            # optimised app: src-tauri/target/release/computer-agent.exe
npm run tauri build                    # installers (MSI + NSIS); the Tauri CLI downloads WiX/NSIS on first run
npm test                               # interface tests (Vitest)
cd src-tauri && cargo test             # engine tests (safety rules, file rules, validation, parsing)
```

CI (`.github/workflows/ci.yml`) type-checks, lints (`clippy -D warnings`), tests and audits dependencies on every push and pull request. Releases build only from `v*` tags (or by hand), after CI passes, as draft releases you publish yourself.

`scripts/mock_llm.py` and `scripts/mock_mcp.py` are a scripted OpenAI-compatible model and a minimal MCP server. Use them to try every flow without a real model or API key:

```bash
python scripts/mock_llm.py
```

Then, in Models → Add provider, choose "Custom OpenAI-compatible" with the address `http://127.0.0.1:8765` (the app adds `/v1` itself).

The mock also streams reasoning: send a message containing "think" (inline `<think>` tags, DeepSeek-R1 style) or "reason" (`reasoning_content` deltas).

`cargo test -- --include-ignored` also runs a live address test against the mock.

## What you get

### Chat
- **Organising chats:** folders, pinning, rename, search, date groups (Today, Yesterday, Previous 7 days…) and "Show more" paging.
- **Per chat:** fork, branch from any message, edit-and-resend, regenerate, stop, delete.
- **Per message:** copy (with a check mark), read aloud, thumbs up/down, time, token count, speed (tokens per second) and time to first token.
- **Code and maths:** syntax highlighting for every common language (light, dark and high-contrast colours), a language label, line count, wrap toggle and copy button; LaTeX maths with KaTeX; tables, quotes and links that open in your browser.
- **Live status:** a waiting line with elapsed time while the model reads the conversation ("Loading model into memory" for the built-in engine), then live tokens per second while it writes.
- **Scrolling like ChatGPT and Claude:** the view follows a streaming reply only while you are at the bottom; scroll up by any means and it stays put until you press "Follow reply".
- **Replies survive navigation:** switch chats or pages mid-reply and come back to the live text; the sidebar marks chats that are still replying.
- **Documents:** attach PDF, Word, PowerPoint, Excel, text, Markdown, CSV, JSON or code files (button, drag and drop, or paste). For each, choose:
  - **Search it (embedding):** split into passages and indexed with your embedding model; each question pulls in the best matches, cited as [1], [2].
  - **Send the full text:** the whole document goes with your message.

  "Ask me each time", or a fixed choice, is a setting. The composer shows a context meter so you can see how full the model's context is.
- **Chat settings:** per-chat instructions, temperature and maximum reply length.
- **Keyboard:** Esc stops a reply; ↑ in an empty composer edits your last message.
- **Selecting many chats:** the check-box button in the sidebar (or Ctrl+click, Shift+click for a range) selects chats to pin, unpin, move to a folder, export as one Markdown file, or delete.
- **Think switch:** the composer shows a Think toggle only for models that can reason. Off asks the model to skip thinking, in each server's own dialect:
  - OpenAI: `reasoning_effort` (`none` or `minimal` on GPT-5 models)
  - OpenRouter: `reasoning.enabled`
  - Ollama: `think`
  - llama.cpp, vLLM, SGLang and LM Studio: `chat_template_kwargs.enable_thinking`
  - Anthropic: extended thinking on or off

  If a server rejects one of these fields, the request is sent again without it.
- **Live reasoning:** a "Thinking · 4s" block streams the model's reasoning, then folds away when the answer starts. Works with `reasoning_content`/`reasoning` fields, Anthropic thinking and inline `<think>` tags. Reasoning effort (Off, Low, Medium, High) is a setting, and so is hiding reasoning.
- **Streaming replies,** with a typing indicator, tool-call drawers and a "Latest" button when you scroll up.
- **Images:** attach images, paste them, or attach a screenshot of your screen.
- **Personas:** instructions you set per chat and globally.
- **Slash commands:** `/agent`, `/chat`, `/new`, `/settings`.
- **Sub-agents (hybrid mode, off by default):** the chat model can hand self-contained tasks to other models with `run_subagents` (several at once), see what is available with `list_models`, and look things up in the chat's documents with `search_documents`. You choose which models it may call and can keep it to local models only. Each sub-agent shows as its own drawer in the reply.
- **Full access (opt-in, with a warning):** a fourth autonomy level where the agent skips plan approval and confirmations, and chat tools (including shell commands) run without asking. Passwords, sign-in screens, CAPTCHAs and off-limits apps still hand control back to you, a banner shows while it is on, and everything is still recorded in the audit log.
- **Model picker** that shows each model's capabilities and whether data leaves your computer.
- **Model switching:** a notice appears when the new model can't do something the chat relies on, such as reading images.
- **Export** to Markdown, JSON, a standalone web page or PDF (through the print dialog's "Save as PDF"), each with a save dialog; or copy the whole chat as Markdown.

### Computer agent
- **How it works:** you approve an editable plan, then each step runs observe → decide → Checker → act → verify.
- **Seeing the screen:** vision models get screenshots with numbered element tags. Text-only models get an "Eyes" model or the Windows accessibility text map.
- **Ways to act, in order of preference:** direct tools (files, clipboard, web, shell), then accessibility actions (pressing controls without the mouse), then pixel clicks.
- **Floating bar:** an always-on-top bar shows the step, live cost, pause/resume and Stop. A highlight appears where the agent is about to click.
- **Yielding to you:**
  - moving the mouse pauses the agent
  - so does flicking the pointer into the top-left screen corner
  - logins, two-step codes and CAPTCHAs are handed to you
- **Tools** (all work on Windows, macOS and Linux):
  - files: list, read, write, move, rename, create folder, delete, copy (every change can be undone)
  - search: file names, and text inside files (`grep_files`)
  - file details with SHA-256 checksum
  - zip and unzip (never overwrites; blocks "../" paths)
  - web search, page reader, HTTP requests to APIs (GET is read-only; other methods ask), downloads
  - processes: list, and stop (always asks)
  - an exact calculator
  - clipboard, system info, open a file, folder or link, notifications
  - shell commands (sandbox first)
  - your plugins and MCP tools
- **Data protection (fixed engine rules):**
  - file tools read and write only inside your allowed folders; paths with `..` are refused and links are followed before checking
  - keys and passwords are never readable: `.ssh`, `.aws`, `.gnupg`, browser profiles, Windows credential folders, `.env` files, key and keystore files (`.pem`, `.key`, `.pfx`, `.kdbx`…) and the app's own data folder
  - once a task or chat has read local data (files, clipboard, documents, command output), any request that leaves the computer (web search, page fetch, HTTP, download, a link with a query) asks first
  - devices on this computer or your local network need approval; cloud metadata addresses are always blocked, including through redirects
  - opening programs or scripts (`.exe`, `.bat`, `.ps1`, `.lnk`…) always asks
  - if content tries to instruct the AI, every further tool call asks, even with Full access
  - images in replies are not loaded from the internet (they become links), and the interface runs under a strict Content Security Policy
- **Limits and recovery:** cost, step and time caps; loop detection; retry, then one alternative, then stop with an explanation. A command or plugin that runs past its time limit is stopped with everything it started.
- **Queue and resume:** one task controls the desktop at a time; resume after a crash.
- **Activity log:** screenshots for every step, an undo window for file changes, and saved playbooks.

### Extensions
- **MCP servers:** add any Model Context Protocol server. It can be a local program (stdio) or a web address (streamable HTTP).
  - Presets are included, or add a custom one.
  - Secret headers and environment values are kept in the OS credential store.
  - Saving runs a live connection test that lists the server's tools.
  - Permission per server: Always allow, Ask first or Off.
- **Plugins:** a folder with a `plugin.json` that declares tools. Each tool runs a program you choose.
  - Create one from the template, import a folder, enable it and set its permission.
  - Test any tool from the UI.
- **Built-in Linux VM (recommended sandbox):** one click in Extensions → Linux sandbox downloads a 10 MB Linux system (Buildroot + BusyBox) and switches it on.
  - It runs inside the app in the v86 emulator (WebAssembly), in a hidden window and a background worker. No WSL, Docker, Hyper-V or admin rights; same on every OS.
  - Each downloaded file is checked against a pinned SHA-256 checksum, so a corrupted or altered file is refused.
  - It boots in about 3 seconds, has no network at all, and stops itself when idle (configurable).
  - `/work` mirrors the shared folder in both directions: files go in before each command, and new, changed or deleted files come back.
  - Each command has a time limit enforced inside the VM. A VM that stops responding is restarted.
  - A small terminal on the page lets you try it.
- **Other sandboxes:** a WSL distribution or a Docker image (`alpine:3.20`, no network by default) also work.
- **Without a sandbox,** commands run in PowerShell (or `sh`) behind an approval gate that applies at every autonomy level.
- **Tools and permissions:** one place to set Allow, Ask or Off for every tool.

### Models
- **Local addresses:** plain `http://` works for this computer and your home network (192.168.x, 10.x, 172.16–31.x, Tailscale 100.64.x, `my-pc`, `*.local`, `*.lan`). These count as "local" for local-only mode. Plain http to a public address gets a warning, not a block.
- **Address fixing:** "Test connection" and "Add" find the address that actually answers. A missing `/v1` is added, a pasted `/chat/completions` is removed, and Ollama works with or without `/v1`.
- **Model lists:** every OpenAI-compatible provider's `/models` list is fetched (searchable and paged), and you can type any model name by hand.
- **Sources:**
  - cloud presets: OpenAI, Anthropic, OpenRouter, Gemini, Groq, Mistral, Together
  - any OpenAI-compatible endpoint
  - one-click detection of Ollama, LM Studio, llama.cpp, Jan and vLLM
- **Test connection** before you save a provider.
- **Capability self-test** on each model: tool calling, image reading and speed. Embedding models are tested by asking for a vector.
- **Your models:** one compact row per model with capability icons (vision, tools, reasoning, audio, embedding, local or cloud), context, measured speed and price. Search, filter chips, sorting, paging, and a switch to hide prices. Select several to test, remove, or allow as sub-agents.
- **Editing a model:** each capability is an on/off switch showing what the test found, with "reset" to go back to it.
- **Roles:** Planner, Doer and Eyes, plus the built-in Checker and a fallback model.
- **Built-in llama.cpp engine** (Models → Built-in engine): run GGUF models with nothing else installed.
  - **Download:** one click downloads the official `llama-server` (release b11685) build for your system. Choices are Vulkan (NVIDIA, AMD, Intel), CPU, CUDA 12 or Apple Metal, and each file is checked against GitHub's published SHA-256.
  - **Import:** import a `.gguf` file, or a whole folder. The file's header is read to confirm it is a real GGUF model and to get its architecture and trained context. A vision projector (`mmproj-*.gguf`) next to the model is found automatically, or you can pick one; models with a projector can read images. Files stay where they are and nothing is copied.
  - **Running:** models appear as a normal local provider. The server starts, or switches model, when a chat or the agent uses one; it waits for the model to load, unloads it when idle, and stops when the app closes. It runs with `--jinja` (tool calling) and `--reasoning-format deepseek` (separate reasoning stream).
  - **Hardware:** the engine lists every device it can use (for example "Vulkan0: AMD Radeon Graphics, integrated" and "Vulkan1: NVIDIA GeForce GTX 1650, dedicated") with free memory. Choose Automatic, CPU only, one GPU, or several; with several, split by layer or row, set each GPU's share, or pick the main GPU. Layers on the GPU and MoE experts on the CPU are adjustable.
  - **Performance:** context size (default and per model), flash attention, K and V cache types (q8_0 halves cache memory), batch and micro-batch sizes, CPU threads, parallel slots, mlock and no-mmap.
  - **Sampling defaults:** temperature, top-p, top-k, min-p and repeat penalty, each "model default" unless you set it.
  - **Server:** idle unload, chat and embedding ports, extra arguments. Saving reloads the loaded model with the new settings.
  - **Embeddings:** one-click downloads of Nomic Embed v1.5, BGE Small, EmbeddingGemma 300M or Qwen3 Embedding 0.6B, checked against pinned SHA-256 hashes. Embedding GGUFs you import are recognised from their header. They run on a second server (`--embedding`) next to the chat model, so documents and chat don't unload each other.
- **Ollama manager:** hardware check, a memory-fit rating, a disk check, license badges, and download progress bars.
  - Download any Ollama model by name.
  - If Ollama isn't found, enter its address (another port, or another computer).
  - Detection also checks `OLLAMA_HOST`, LocalAI, KoboldCpp, text-generation-webui and GPT4All.

### Audit log
- **What it records:**
  - every state-changing action you take (one hook on every engine command)
  - every AI action: chat replies with model, tokens and cost; each tool call with arguments, permission and result; each agent step with Checker verdict and screenshot; approvals and denials
  - app events: start, VM boot, pruning
- **Secrets are never stored:** API keys, tokens, passwords, headers and environment values are replaced by `[redacted]`, and images and large file bodies are shortened.
- **Tamper evidence:** each record carries a keyed hash (HMAC-SHA-256, key in the OS credential store) linking it to the record before it. "Verify integrity" finds any edited or deleted record. The log keeps tool calls and commands, not the text of your messages.
- **Finding things:** search, filters (area, who, result, time range), one session at a time, and pagination with a page-size choice.
- **Export and retention:** export to CSV or JSON. Retention is a setting; the default is 365 days, and 0 keeps records forever.

### Stats
- **Totals for 7, 30 or 90 days, or all time:** replies, tokens in and out, average speed, average time to first token, cost, agent tasks, documents and problems.
- **Charts:** tokens per day (output and input) and replies by hour of day.
- **Per model:** replies, tokens, average and peak tokens per second, time to first token, cost and last use; sortable and searchable.
- **Tools:** how often each tool and sub-agent was used, and how often it failed.

### Personalisation and UX
- **Window and navigation:**
  - the app draws its own title bar
  - command palette (Ctrl+K)
  - collapsible sidebar (Ctrl+B)
  - keyboard shortcuts for everything
- **Look:**
  - your app name and assistant name
  - accent colour
  - corner style (sharp, rounded or round)
  - density and text size
  - light, dark, system and high-contrast themes
- **Settings:**
  - Basic, Advanced and Expert layers
  - one-click profiles
  - searchable sections
  - import, export and reset
- **Validation:** every form checks input inline, and the engine checks again before saving.
- **No native browser controls:** dropdowns, number fields, checkboxes, sliders, colour pickers, menus, confirmations and prompts are the app's own keyboard-accessible components.
- **Motion:** subtle page, message, menu and modal transitions; button press feedback; spinners, progress bars and skeleton loaders wherever something takes time. All of it is off with the system's reduced-motion setting.
- **Lists that grow:** Activity, the audit log, model lists, tools and the chat sidebar all have search, filters and pagination.
- **Layout:** the window itself never scrolls; the sidebar and the page scroll on their own.
- **Accessibility:** full keyboard access, ARIA labels and reduced-motion support.

## Writing a plugin

```json
{
  "name": "word-count",
  "version": "1.0.0",
  "description": "Counts words in a text.",
  "tools": [{
    "name": "count_words",
    "description": "Counts the words and characters in a piece of text.",
    "parameters": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
    "command": "python",
    "args": ["tool.py"],
    "risk": "routine",
    "timeout_secs": 20
  }]
}
```

- **Input:** arguments arrive as JSON on stdin and in the `PLUGIN_ARGS` environment variable.
- **Output:** print the result to stdout. A non-zero exit code is reported as an error.
- **`risk`:**
  - `routine` runs freely
  - `risky` asks in Balanced mode
  - `gate` always asks

## Architecture

There is a **Rust engine** (`src-tauri/src`) and a **React interface** (`src`). Every safety decision lives in the engine.

| Module | Responsibility |
| --- | --- |
| `llm.rs` | OpenAI-compatible and Anthropic connections: streaming, tool calls, retries, local-only block |
| `models.rs` | Providers, models, capability self-test, runtime detection, hardware check, Ollama manager |
| `screen.rs`, `a11y.rs`, `input.rs` | Capture and settle, numbered tags, secret masking; UI Automation worker; mouse and keyboard |
| `safety.rs` | The Checker: fixed gates, blocked apps and sites, hand-offs, hidden-instruction detection, approval matrix |
| `agent.rs` | The agent loop, roles, limits, recovery, queue, focus handling, crash resume |
| `toolbox.rs` | Shared tools (web search, fetch, file search, system info, shell, open path) and the extension registry |
| `mcp.rs` | MCP client over stdio and streamable HTTP |
| `plugins.rs` | Folder plugins: manifest validation, execution, template, import |
| `sandbox.rs` | Sandbox selection; WSL and Docker detection and execution with time limits |
| `microvm.rs`, `src/vm/` | Built-in Linux VM: verified download, lifecycle, command bridge, shared-folder sync |
| `audit.rs` | Hash-chained audit trail: recording, redaction, query, export, verify, retention |
| `llamacpp.rs` | Built-in llama.cpp: verified download, GGUF header reader, projector pairing, server lifecycle |
| `docs.rs` | Chat documents: text from PDF/Office/text files, chunking, embeddings, vector search |
| `subagents.rs` | Hybrid mode: run_subagents, list_models and search_documents tools |
| `xtools.rs` | Cross-platform tools: text search, file info, processes, calculator, HTTP, download, copy, zip |
| `files.rs` | File tools through the undo holding area |
| `validate.rs` | Engine-side validation of settings, providers, models, MCP servers and shortcuts |
| `chat.rs`, `commands.rs`, `ext_commands.rs` | Chat streaming and the commands the interface calls |
| `db.rs`, `settings.rs`, `secrets.rs` | SQLite with versioned migrations and backups; layered settings; OS credential store |

**Production behaviour:**
- **One running copy:** opening the app a second time focuses the existing window.
- **Crash log:** written to `<app data>/logs/crash.log`; held keys are released when the app crashes.
- **Safe startup:** windows open only after the engine is ready.
- **Contained errors:** a broken view shows a recovery screen instead of a blank window.
- **Timeouts:** every external process and network call has one.

## Known limits

- **Accessibility view:** Windows only (UI Automation). macOS and Linux use vision; the System page says so.
- **Data at rest:** stored in plain SQLite in the app data folder. API keys and MCP secrets are in the OS credential store; full database encryption and an app lock are not built yet.
- **Pausing when you take over:** the agent detects your mouse movement but not your typing.
- **Not built yet:**
  - voice and omni models
  - "show me once" recording
  - scheduled playbooks
  - long-term memory
  - team features
  - background desktops
- **Built-in VM limits:** it emulates a 32-bit x86 PC, so it suits shell work (BusyBox tools, scripts, text processing) rather than heavy compiling. It has no package manager or network by design.
- **Documents:** scanned PDFs without a text layer can't be read (attach them as images instead). Embedded documents must be attached again after you change the embedding model.
- **Sub-agents** need a main model with tool calling; sub-agents themselves get no tools.
- **Built-in engine:** runs one chat model at a time (plus one embedding model); switching chat models reloads the server. It is unloaded when the app closes normally. After a crash, end any leftover `llama-server` process in Task Manager.
- **Elevated windows:** admin-level windows can only be controlled when the app runs elevated (a Windows rule).

## License

Licensed under the [Apache License, Version 2.0](LICENSE). Copyright 2026 Anand; see [NOTICE](NOTICE).
