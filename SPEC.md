# SPEC

## 1. Scope
This tool converts Qwen-like JSON conversation exports into Markdown files following a built-in output contract (compatible with the previous `chatformat.txt` structure).

## 2. CLI

## Command
- `qwen-markdown-converter convert -i <input> [-o <output>] [--progress true|false]`
- Drag-drop mode (Windows friendly): `qwen-markdown-converter <path1> [path2 ...]`

## Parameters
- `-i, --input <path>`: input JSON file or directory
- `-o, --output <path>`:
  - directory path for batch/dir conversion
  - `.md` file path only valid for single-session output
- `--progress <bool>`: enable or disable indicatif progress bar

## Validation
- missing input -> error
- multi-input with markdown file output path -> error

## 3. Input Model

## Single Session
- top-level object (non-wrapper), or top-level array with one session object

## Wrapped Large JSON
- top-level object:
  - `success: bool`
  - `request_id: string`
  - `data: [session, session, ...]`

## Directory Mode
- recursively scans `*.json`

## 4. Markdown Format

## Sections
- `## Metadata`
- `### Run Settings`
- `## Conversation`

## Metadata fields
- `Model`
- `Tags` (normalized)
- `Conversation ID`
- `User ID`
- `Request ID` (if wrapped input)
- `Chat Type`
- `Sub Chat Type`
- `Source`
- `Generated At (UTC)`

## Messages
- user: `### 🧑‍💻 User`
- assistant: `### 🤖 Assistant`
- optional assistant split:
  - `#### 🤔 Thought Process`
  - `#### 💡 Response`

## Branching
- for one user with multiple assistant children, user message is duplicated and paired with each assistant reply (regen expansion)

## 5. Field Semantics
- `tags`: extracted from session `meta.tags`, supports list/string; normalized by:
  - trim whitespace, wrapping quotes/backticks, leading `#`
  - delimiter precedence: comma, semicolon, then whitespace
  - case-insensitive de-duplication while preserving first spelling
- `model`: first available assistant `modelName`, fallback `model`, fallback `unknown`
- timestamps:
  - source from message/session timestamp fields
  - millisecond timestamps converted to seconds
  - written back to output markdown file mtime

## 6. Concurrency Strategy
- Uses `rayon` with thread count from `std::thread::available_parallelism()`
- Directory/file batch conversion: processed in parallel with `ParallelIterator`
- Wrapped large JSON session output:
  - chunked by `1000` sessions per batch
  - each batch written in parallel

## 7. Error Handling

## Behavior
- Per-file parse/write failure is collected and logged to `error.log`
- Batch continues after per-file failure
- Process exits with code `1` if any failure occurred; `0` when all succeed

## Error code table
- `0`: success, no failed file
- `1`: one or more file-level failures, details in `error.log`

## 8. Progress and Logging
- Progress bar via `indicatif`:
  - file-level progress in batch modes
  - session-level progress in large wrapped single-file mode
- Logging:
  - controlled by `RUST_LOG`
  - default level `info`
  - `debug` includes extracted metadata details (including tags)

## 9. Test Coverage Targets
- single small JSON conversion assertion
- wrapped large JSON split file count equals `data.len()`
- directory mode output tree validation
- missing meta -> fallback tag strategy
- drag-drop style invocation (positional path mode)

## 10. Performance Target
- Reference target: `<= 30s` for 10k files on 8C/16G environment (depends on storage and JSON size)
- Current implementation design supports this through:
  - parallel processing
  - chunked large-batch writing
