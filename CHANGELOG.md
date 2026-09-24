# Changelog

All notable changes to MarginPost are documented in this file.

The project follows Semantic Versioning after the first public release.

## [Unreleased]

## [0.3.0] - 2026-09-24

### Added

- Markdown AST-backed structural comparison.
- Previous/next change navigation and keyboard review controls.
- Undo for the most recent in-session review decision.
- Workspace search and pending/all Change Set filters.
- Clear conflict, stale, and superseded Change Set states.

## [0.2.0] - 2026-09-24

### Added

- Ad-hoc signed macOS `.app` and `.dmg` build configuration.
- Claude Code, Codex, and Cursor integration examples.
- Two-minute product demonstration source and rendered video.
- Windows and Linux CI jobs for build and test validation.
- Release, tag, and changelog preparation.

### Changed

- Cross-platform support is described as provisional until the new CI jobs
  pass on the public repository.

## [0.1.1] - 2026-09-24

### Fixed

- Corrected the product position from pre-write interception to post-write
  detection with conflict-safe review application.
- Added Markdown create, delete, and rename review semantics.
- Rejecting a newly created file now removes it instead of leaving an empty
  file.
- Sequential edits now mark older Change Sets as superseded.

### Added

- Public Agent Hook documentation.
- Change Set lifecycle and trust-boundary documentation.
