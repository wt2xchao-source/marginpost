# Agent Hooks

MarginPost detects Markdown changes from the local filesystem. Hooks are
optional and only add a self-reported source label or a session-finished
notification. They do not grant an agent permission to write files and they do
not intercept writes before they reach disk.

## Command Interface

Use the executable inside the installed app:

```text
/Applications/MarginPost.app/Contents/MacOS/agent-markdown-reviewer
```

Attribute an upcoming Markdown write:

```text
agent-markdown-reviewer --marginpost-attribute "<source>" "<absolute-path>"
```

Notify the running app that an agent session ended:

```text
agent-markdown-reviewer --marginpost-notify "<source>"
```

Attribution is best effort. MarginPost matches the explicitly reported absolute
path to a filesystem event within a short local time window. Unmatched changes
remain correctly labeled as external changes.

## Claude Code

Add hooks to Claude Code `settings.json`. Replace the executable path if
MarginPost is installed elsewhere.

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Write|Edit|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "f=$(jq -r '.tool_input.file_path // empty'); if printf '%s' \"$f\" | grep -iqE '[.](md|markdown)$' && pgrep -fq 'agent-markdown-reviewer'; then '/Applications/MarginPost.app/Contents/MacOS/agent-markdown-reviewer' --marginpost-attribute 'Claude Code' \"$f\"; fi; exit 0"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "'/Applications/MarginPost.app/Contents/MacOS/agent-markdown-reviewer' --marginpost-notify 'Claude Code'"
          }
        ]
      }
    ]
  }
}
```

The example requires `jq` for reading Claude Code's hook payload.

## Codex

Codex does not currently expose the same stable write-hook contract used by
the Claude Code example. Use a project instruction that runs the attribution
command immediately before each Markdown write:

```text
Before modifying a .md or .markdown file, run:
"/Applications/MarginPost.app/Contents/MacOS/agent-markdown-reviewer" \
  --marginpost-attribute "Codex" "<absolute-file-path>"
```

This is an integration convention, not a security boundary. Filesystem
monitoring remains the source of truth.

## Cursor

For agent commands or tasks that can run a pre-write shell step, invoke:

```text
"/Applications/MarginPost.app/Contents/MacOS/agent-markdown-reviewer" \
  --marginpost-attribute "Cursor" "<absolute-file-path>"
```

Cursor does not provide a universal post-edit hook for every editing path.
When no hook is invoked, MarginPost still captures the change as an unlabeled
external modification.

## Troubleshooting

- Keep MarginPost running with the target workspace open.
- Pass an absolute Markdown path.
- The path must end in `.md` or `.markdown`.
- Source labels are not persisted when no matching filesystem event arrives.
- A hook notification never proves that a file changed; the filesystem event
  and captured Change Set remain authoritative.
