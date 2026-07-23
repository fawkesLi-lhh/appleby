# Appleby Feature Decisions

Only `Required` and `Approved` features are valid production features. Unlisted and `Pending` features do not justify keeping production code.

## Required: Minimal ReAct Agent

Source: `docs/agent工程学指南/1-ReAct.md`

- Accept user input.
- Persist the user message in conversation context.
- Call the model with the current context.
- Receive assistant text and tool calls.
- Execute requested tools.
- Persist tool results linked to their tool calls.
- Continue the model/tool loop until no tool call remains.
- Present the final assistant answer to the user.

## Approved: Runtime Support

- File-backed conversation context using `context.jsonl`.
- Load recent context on normal startup.
- Explicit fresh-context startup that archives the previous context.
- Configurable OpenAI-compatible API key, base URL, and model.
- Configurable system prompt.
- Local tools: `Bash`, `Read`, `Write`, and `Edit`.

## Approved: TUI Frontend

- Accept user input in the terminal, including multiline input.
- Present committed user and assistant messages.
- Present tool execution and failure information.
- Present Agent errors to the user.
- Show an activity indicator while the model or a tool is running.
- Allow manual scrolling through the current TUI transcript.
- The TUI may show a clearly marked truncated tool-output preview only when the complete result remains available at a traceable destination.
- On an exit request, allow the current Agent turn to finish and then shut down.
- Restore terminal state when the TUI exits.

## Pending: TUI Details

The following are not yet approved and do not independently justify production code:

- Historical conversation rendering.
- Cancellation of the current Agent turn.

## Pending: Extended Agent Architecture

The Specialist/Synthesizer dual-loop, infinite-memory, summary, scratchpad, memory-tool, and skill-organization designs are not part of the current required MVP until explicitly approved here.
