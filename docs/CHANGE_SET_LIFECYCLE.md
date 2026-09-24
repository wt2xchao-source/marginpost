# Change Set Lifecycle

MarginPost observes the filesystem after another tool writes, creates, deletes,
or renames a Markdown file. It then captures the last known workspace baseline
and the observed candidate as a Change Set.

## Statuses

- `pending`: the latest reviewable candidate for a file chain.
- `superseded`: a newer candidate replaced this snapshot. It remains visible
  for context but cannot be applied or discarded.
- `stale`: the disk no longer matches the candidate that was opened for review.
  MarginPost refuses to overwrite the newer disk state.

When the same file changes repeatedly before review, every new Change Set keeps
the original baseline. The newest Change Set records the IDs it replaces, and
older entries identify the newer Change Set through `supersededBy`.

Resolving or discarding the newest Change Set closes the entire superseded
chain. This prevents an older snapshot from being applied over a newer edit.

## File Operations

Creation, deletion, and rename are explicit file-operation decisions, separate
from content decisions:

- Rejecting a newly created file removes it instead of leaving an empty file.
- Rejecting a deletion recreates the file from the reviewed result.
- Accepting a deletion keeps the path absent.
- Rejecting a rename restores the previous path.
- Accepting a rename keeps the new path.

Every disk mutation verifies the current candidate hash or verifies that the
expected path is still absent. If either condition fails, MarginPost stops with
a conflict and does not overwrite the newer state.

## Trust Boundary

MarginPost is not a pre-write sandbox. External tools write to disk first.
MarginPost detects the resulting filesystem event, creates a review record,
and protects the later review decision with hash and path-existence checks.
