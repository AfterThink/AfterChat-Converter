# Memory (Project Conventions)

This file records stable decisions from recent iterations.

## Output behavior
- No external template file is required.
- Markdown format is generated from built-in structure.
- Windows drag-and-drop is a primary usage mode (`exe <json-path>`).

## File naming
- Prefer session `title` for file name.
- Fallback order: `title` -> `id` -> source file stem.
- For large wrapped exports, no numeric prefix is used.
- Duplicate names are disambiguated with suffixes (`-2`, `-3`, ...).

## Metadata conventions
- `Model` must be written as `models/<modelname>`.
- If model already starts with `models/`, keep it unchanged.
- Thinking content should be rendered when present under:
  - `reasoning_content`
  - thinking phases in `content_list`
  - nested thinking summaries in `content_list[].extra`

## Robustness
- Parser tolerates `null` for list/map-like fields by treating them as empty.
- Invalid items inside array payloads are skipped with warnings instead of aborting whole conversion.
