# The assistant sidebar

SolidCraft has a chat sidebar on the right of the window. You describe what you want or ask a
question; the assistant looks at your design and answers, or models for you with the same
commands the toolbar uses. Everything it changes appears in the window at once, and one click
undoes a whole answer.

Open it with the speech-bubble button in the title bar or **Ctrl+Shift+A** (the same key closes
it, also from inside its text box). The sidebar is part of the desktop app; the browser version
shows no assistant.

## Choosing who answers

Click the gear in the sidebar's header.

| Service | What you need |
|---|---|
| **Anthropic (Claude)** | An API key. Paste it in the settings or set `ANTHROPIC_API_KEY`. |
| **OpenAI-compatible** | The address and model name of the service (`https://api.openai.com/v1`, or a local server such as Ollama at `http://localhost:11434/v1` or LM Studio). A key if the service wants one (`OPENAI_API_KEY` works too). |
| **External agent** | A coding agent installed on your computer: OpenCode, Claude Code (through an ACP adapter), Gemini CLI, Codex, Antigravity (`agy`)… pick a preset or enter the program and its arguments. |

Plain `http://` is only used for the local computer; a server on your network needs **Allow
unencrypted HTTP to this address**. Redirects are never followed, so a key never goes to a host
you did not enter.

### External agents

An external agent is a program SolidCraft starts and talks to over the
[Agent Client Protocol](https://agentclientprotocol.com) (ACP, JSON-RPC on the program's standard
input and output), or, for programs without ACP, runs once per message and shows what it prints.
The program models through SolidCraft's own [MCP server](mcp.md): SolidCraft starts a loopback
connection with its own random token, hands the agent `solidcraft-cli mcp --connect` as its MCP
server, and everything the agent does arrives through the same checks as the in-app assistant.
`solidcraft-cli` is found next to the app or on `PATH`; the settings can name it.

The presets are starting points: programs change their switches between releases. If one does
not start, check the program's own documentation and edit the command and arguments.

Agents that need a Node runtime are started with `npx`; Flatpak and Snap installations can't
start programs outside their sandbox.

## What the assistant can do

- Explain the design: the timeline, what each feature does, how the parameters drive it.
- Find out why a feature fails and fix it.
- Build and change parts: sketches, features, parameters, patterns, anything the commands do.
- Check its work: it measures, inspects and (with a vision-capable model) looks at a picture of
  your view. **📷** attaches the current view to your next message.

It sees a summary of the open design with every message (parameters, timeline with errors,
bodies, sketches, selection) and can ask for more through its tools. It can use every MCP tool,
including `crash_reports`.

### Quick actions

Type `/` in the message box:

| | |
|---|---|
| `/explain` | Explain the design, or the selection |
| `/fix` | Diagnose and fix features that fail (the failing features are listed for it) |
| `/params` | Review the parameters; suggestions only |
| `/check` | Check the design without changing it |
| `/new` | Start a new conversation |
| `/help` | List the quick actions |

Text after a quick action is added to it (`/fix keep the wall thickness`).

### Tool cards

Each command the assistant runs is a card in the conversation: the command, its parameters and
what came back. Failures open by themselves. **Select feature** selects the feature a card
created or changed.

## Staying in control

- **One undo per answer.** A turn that changed the design ends with **Undo these changes**. It
  puts the design, the undo history and the active sketch back to before your message. If you
  edited the design afterwards, the button says so and asks for a second click.
- **Files wait for your yes.** Commands that read or write files, open another design or start a
  new one (`file.*`, `doc.*`, any command given a file name) show an **Allow / Deny** card with
  the exact command and path, for the in-app assistant and external agents alike. **Stop**
  declines everything waiting.
- **Folders.** The assistant reaches files only in the design's own folder, the app's recovery
  folder and its private scratch folder, plus the folders you list in the settings. A file
  outside them is refused even if you allow the command.
- **Fixed rules, not requests.** The assistant can't click the interface, change preferences or
  quit the app: only running commands, reading the design, moving the camera and rendering a
  picture are allowed.
- **A step limit.** A turn ends after 24 model requests, so a loop can't run on (and cost) for
  long.
- **Stop** ends a turn after the step in progress; a connection that does not answer is given up
  on after a few seconds.

## Conversations

Conversations are kept per design in the assistant's data folder, not in the design file, so
sharing a design never shares a chat. The clock button lists earlier conversations of the
open design; **Export as text…** writes one as Markdown. External agents keep their own memory
for as long as the program runs; **New chat** starts a new one.

## Privacy and where things are stored

What leaves your computer is what you send to the service you chose: your messages, a summary of
the open design with each message (its file *name*, parameters, features, bodies, sketches; no
folder paths), the results of the tools the assistant calls, and pictures you attach. Nothing is
sent until you send a message. SolidCraft contacts no other server for this.

With an external agent, the program you picked decides where its data goes.

Settings (including keys and the environment values of agent programs) are stored in
`<data folder>/assistant/settings.json`, readable only by you (`0600`; the user's own folder
permissions on Windows). They are not encrypted. The environment variables
`SOLIDCRAFT_ASSISTANT_DIR`, `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are read; keys are never
logged, never put in an address, and removed from error messages.

## For scripts and tests

`ui.assistant {action}` on the [control channel](control-protocol.md) drives the sidebar:
`open`, `close`, `toggle`, `state`, `send {text}`, `stop`, `confirm {index?, allow}`, `undo_turn`
and `new_chat`. The state never contains keys, and settings are not reachable from there: a
control client must not be able to point a saved key at another server.
