#!/usr/bin/env python3
"""Cherry Studio 备份 → AfterChat Markdown（zip）

输入：Cherry Studio 导出的 JSON，或包含 `data.json` 的 zip
输出：一个 zip

    chat-export-cherry-all-<毫秒时间戳>.zip
    ├── <助手名>/<YYYYMMDD-HHmmss>-<标题>.md
    └── export-failures.md        （仅当有主题没有任何消息时）

输出格式遵循 `CHATFORMAT-CONVERTER.md`（与 Rust 版 `src/main.rs` 同一口径）。

用法：
    python convert-json-to-markdowns.py <备份.json|备份.zip> [-o 输出目录|输出.zip]
    python convert-json-to-markdowns.py            # 等价于 data.json
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import zipfile
from datetime import datetime, timedelta, timezone
from pathlib import Path

# ── 契约常量 ────────────────────────────────────────────────────────────────
EXPORT_PREFIX = "chat-export-cherry-all"
ENTRY_TITLE_MAX = 80

LOCAL_TZ = datetime.now().astimezone().tzinfo or timezone.utc

# ── 标题 → 文件名 ───────────────────────────────────────────────────────────


def sanitize_filename(name: str) -> str:
    """去掉 Windows 非法字符（含换行/制表符）"""
    return re.sub(r'[\\/*?:"<>|\r\n\t]', "", name)


def sanitize_path_component(name: str, fallback: str) -> str:
    cleaned = sanitize_filename(name).strip()
    if cleaned == "" or cleaned in (".", ".."):
        return fallback
    return cleaned


def truncate_chars(text: str, limit: int) -> str:
    return text if len(text) <= limit else text[:limit]


# ── 正文标题 → 加粗（契约 §5）───────────────────────────────────────────────


def remove_bold_outside_code(text: str) -> str:
    """去掉不在行内代码段里的 `**`（行内代码里的 `**` 是内容，如 glob `**/*.js`）

    行内代码界定用 CommonMark 的 code span 规则：N 个反引号开始，同样 N 个结束。
    """
    out: list[str] = []
    index = 0
    open_ticks = 0
    while index < len(text):
        if text[index] == "`":
            start = index
            while index < len(text) and text[index] == "`":
                index += 1
            run = index - start
            if open_ticks == 0:
                open_ticks = run
            elif open_ticks == run:
                open_ticks = 0
            out.append(text[start:index])
        elif open_ticks == 0 and text.startswith("**", index):
            index += 2
        else:
            out.append(text[index])
            index += 1
    return "".join(out)


def strip_hashes_line(line: str) -> str:
    hashes = 0
    while hashes < len(line) and line[hashes] == "#":
        hashes += 1
    if hashes == 0 or hashes > 6:
        return line

    rest = line[hashes:]
    trimmed = rest.lstrip()
    # `#` 后必须紧跟空白，且后面要有内容
    if trimmed == rest or trimmed == "":
        return line

    inner = remove_bold_outside_code(trimmed).strip()
    if inner == "":
        return line
    return f"**{inner}**"


def strip_hashes(text: str) -> str:
    """`^#{1,6}\\s+(.+)$` → `**$1**`

    - 代码围栏（``` / ~~~）内部原样保留，否则 Python / Shell 的 `# 注释` 会被误改
    - 标题内原有的 `**` 会被吸收，保证整条标题落在一个加粗里
    """
    out: list[str] = []
    fence: str | None = None

    for line in text.split("\n"):
        stripped = line.lstrip()
        if stripped.startswith("```"):
            marker: str | None = "```"
        elif stripped.startswith("~~~"):
            marker = "~~~"
        else:
            marker = None

        if fence is not None:
            out.append(line)
            if marker == fence:
                fence = None
        elif marker is not None:
            fence = marker
            out.append(line)
        else:
            out.append(strip_hashes_line(line))

    return "\n".join(out)


# ── 时间 ────────────────────────────────────────────────────────────────────


def parse_created_at(value):
    """cherry 的 `createdAt`：RFC3339 字符串 / epoch 秒或毫秒数字"""
    if isinstance(value, str):
        text = value.strip()
        if not text:
            return None
        try:
            dt = datetime.fromisoformat(text.replace("Z", "+00:00"))
        except ValueError:
            return None
        return dt.replace(tzinfo=LOCAL_TZ) if dt.tzinfo is None else dt

    if isinstance(value, (int, float)) and not isinstance(value, bool):
        secs = value / 1000.0 if abs(value) >= 1e11 else float(value)
        try:
            return datetime.fromtimestamp(secs, tz=LOCAL_TZ)
        except (OverflowError, OSError, ValueError):
            return None

    return None


def created_at_sort_key(value) -> float:
    """排序用：数字原样返回，字符串转成 epoch 秒（与 Rust 版一致）"""
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return float(value)
    if isinstance(value, str):
        try:
            return datetime.fromisoformat(value.strip().replace("Z", "+00:00")).timestamp()
        except ValueError:
            return 0.0
    return 0.0


def format_time(dt: datetime) -> str:
    """契约 §2：本地时间 `YYYY-MM-DD HH:mm:ss ±HH:MM`"""
    local = dt.astimezone(LOCAL_TZ)
    offset = local.strftime("%z")  # +0800
    if len(offset) == 5:
        offset = f"{offset[:3]}:{offset[3:]}"
    return f"{local.strftime('%Y-%m-%d %H:%M:%S')} {offset}"


def format_local_compact(dt: datetime) -> str:
    return dt.astimezone(LOCAL_TZ).strftime("%Y%m%d-%H%M%S")


# ── 读输入 ──────────────────────────────────────────────────────────────────


def load_root(path: Path) -> dict:
    if path.suffix.lower() == ".zip":
        with zipfile.ZipFile(path) as archive:
            candidates = [
                info
                for info in archive.infolist()
                if not info.is_dir() and Path(info.filename).name.lower() == "data.json"
            ]
            if not candidates:
                raise SystemExit(f"{path} 里没有 data.json")
            entry = min(candidates, key=lambda i: len(Path(i.filename).parts))
            raw = archive.read(entry)
    else:
        raw = path.read_bytes()

    # ChatFormat 允许 UTF-8 BOM，json 模块不接受，先剥掉
    if raw.startswith(b"\xef\xbb\xbf"):
        raw = raw[3:]

    try:
        return json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SystemExit(f"解析 {path} 失败: {exc}") from exc


# ── 渲染 ────────────────────────────────────────────────────────────────────


def render_topic(topic, assistants_map, topic_metadata_map, blocks_map, blocks_by_message):
    """把一个主题渲染成契约格式的 Markdown，并算好 zip 条目名与时间"""
    topic_id = topic.get("id")
    meta = topic_metadata_map.get(topic_id, {})
    topic_name = meta.get("name", "Untitled")
    assistant_id = meta.get("assistantId", "default")

    created_at_dt = parse_created_at(meta.get("createdAt")) or parse_created_at(
        topic.get("createdAt")
    )

    assistant_info = assistants_map.get(assistant_id, {})
    assistant_name = assistant_info.get("name", "Assistant")
    system_instruction = assistant_info.get("prompt", "") or ""

    messages = topic.get("messages") or []
    if not messages:
        return None, {
            "id": topic_id,
            "title": topic_name,
            "reason": "没有消息",
        }

    messages = sorted(messages, key=lambda m: created_at_sort_key(m.get("createdAt")))

    # 契约 §2：推荐的键排在前面，cherry 特有的键随后
    first_model = "Unknown"
    for message in messages:
        if message.get("role") == "assistant" and message.get("model"):
            model_info = message["model"]
            if isinstance(model_info, dict):
                candidate = model_info.get("id")
                if isinstance(candidate, str):
                    first_model = candidate
            elif isinstance(model_info, str):
                first_model = model_info
            if first_model != "Unknown":
                break

    time_str = format_time(created_at_dt) if created_at_dt else "unknown"

    out: list[str] = []
    out.append("## Metadata\n")
    out.append("### Run Settings\n")
    out.append(f"- **Model:** `{first_model}`")
    out.append(f"- **Time:** {time_str}")
    out.append(f"- **Topic ID:** `{topic_id}`")
    out.append(f"- **Assistant:** `{assistant_name}`")
    out.append("")
    out.append("## Conversation\n")

    # 契约 §3：系统提示是对话的第一条消息
    if system_instruction.strip():
        out.append("### ⚙️ System\n")
        out.append(strip_hashes(system_instruction))
        out.append("")

    for message in messages:
        role = message.get("role", "")

        content_blocks = [
            blocks_map[bid] for bid in (message.get("blocks") or []) if bid in blocks_map
        ]
        if not content_blocks:
            content_blocks = list(blocks_by_message.get(message.get("id"), []))
        content_blocks.sort(key=lambda b: created_at_sort_key(b.get("createdAt")))

        # 契约 §3：思考块合并成一段、回复块合并成一段，各只出一个标题
        thoughts: list[str] = []
        responses: list[str] = []
        for block in content_blocks:
            content = block.get("content", "")
            if not content or not content.strip():
                continue
            if block.get("type") == "thinking":
                thoughts.append(content)
            else:
                responses.append(content)

        if not thoughts and not responses:
            continue  # 空消息不要留孤立的角色头

        header = {
            "user": "### 🧑‍💻 User",
            "system": "### ⚙️ System",
        }.get(role, "### 🤖 Assistant")
        out.append(header)
        out.append("")

        if thoughts:
            out.append("#### 🤔 Thought Process\n")
            out.append(strip_hashes("\n\n".join(thoughts)))
            out.append("")
            if role != "user" and responses:
                out.append("#### 💡 Response\n")

        if responses:
            out.append(strip_hashes("\n\n".join(responses)))
            out.append("")

    markdown = "\n".join(out) + "\n" if out else ""

    safe_name = sanitize_filename(topic_name)
    if not safe_name.strip():
        safe_name = "Untitled_Conversation"
    prefix = format_local_compact(created_at_dt) if created_at_dt else "00000000-000000"

    entry_name = (
        f"{sanitize_path_component(assistant_name, 'Assistant')}/"
        f"{prefix}-{truncate_chars(safe_name.strip(), ENTRY_TITLE_MAX)}.md"
    )
    return {
        "entry_name": entry_name,
        "markdown": markdown,
        "epoch": created_at_dt.timestamp() if created_at_dt else None,
    }, None


def unique_entry_name(name: str, used: set) -> str:
    """同一条目名重复时追加 `-2` / `-3`（扩展名保持在末尾）"""
    if name not in used:
        used.add(name)
        return name

    if "." in name:
        stem, _, ext = name.rpartition(".")
        stem, ext = stem, f".{ext}"
    else:
        stem, ext = name, ""

    counter = 2
    while True:
        candidate = f"{stem}-{counter}{ext}"
        if candidate not in used:
            used.add(candidate)
            return candidate
        counter += 1


def build_failure_markdown(source: str, failures: list) -> str:
    lines = ["# Export Failures", "", "## Metadata", ""]
    lines.append("- **Platform:** `cherry-studio`")
    lines.append(f"- **Source:** `{source}`")
    lines.append(f"- **Skipped:** {len(failures)}")
    lines.append("")
    lines.append("这些主题没有任何可导出的消息，因此未生成 Markdown。")
    lines.append("")
    for index, failure in enumerate(failures, start=1):
        lines.append(f"## {index}. {failure['title']}")
        lines.append("")
        lines.append(f"- **Topic ID:** `{failure['id']}`")
        lines.append(f"- **Reason:** {failure['reason']}")
        lines.append("")
    return "\n".join(lines)


# ── 主流程 ──────────────────────────────────────────────────────────────────


def convert(input_path: Path, output=None, source_label: str | None = None) -> Path:
    """`source_label` 是失败报告里显示的来源路径；默认用 Path，但主程序会传入用户原样输入的字符串
    （Path 在 Windows 上会把 `E:/a` 规范化成 `E:\\a`，与 Rust 版不一致）。
    """
    source = source_label or str(input_path)
    root = load_root(input_path)

    indexed_db = root.get("indexedDB", {}) or {}
    local_storage = root.get("localStorage", {}) or {}
    if not indexed_db.get("topics") and not local_storage and not indexed_db.get("message_blocks"):
        raise SystemExit("不是 Cherry Studio 备份：找不到 topics / localStorage / message_blocks")

    # 1. 解析助手与主题元数据（两层嵌套的 JSON 字符串）
    assistants_map: dict = {}
    topic_metadata_map: dict = {}
    try:
        persist_str = local_storage.get("persist:cherry-studio")
        if persist_str:
            assistants_str = json.loads(persist_str).get("assistants")
            if isinstance(assistants_str, str):
                store = json.loads(assistants_str)
                all_assistants = []
                if store.get("defaultAssistant"):
                    all_assistants.append(store["defaultAssistant"])
                assistants_list = store.get("assistants") or []
                if isinstance(assistants_list, list):
                    all_assistants.extend(assistants_list)

                for assistant in all_assistants:
                    aid = assistant.get("id")
                    assistants_map[aid] = {
                        "name": assistant.get("name", "Unknown Assistant"),
                        "prompt": assistant.get("prompt", "") or "",
                    }
                    for topic in assistant.get("topics") or []:
                        tid = topic.get("id")
                        if tid:
                            topic_metadata_map[tid] = {
                                "name": topic.get("name", "Untitled"),
                                "assistantId": aid,
                                "createdAt": topic.get("createdAt"),
                            }
    except (json.JSONDecodeError, AttributeError, TypeError) as exc:
        print(f"解析助手/主题信息失败（非致命）: {exc}", file=sys.stderr)

    # 2. block 索引：按 ID + 按 messageId
    blocks = indexed_db.get("message_blocks") or []
    blocks_map = {b["id"]: b for b in blocks if "id" in b}
    blocks_by_message: dict = {}
    for block in blocks:
        mid = block.get("messageId")
        if mid is not None:
            blocks_by_message.setdefault(mid, []).append(block)

    topics = indexed_db.get("topics") or []
    if not topics:
        raise SystemExit("未找到任何对话主题 (topics)")

    print(f"找到 {len(topics)} 个对话主题，开始转换...")

    # 3. 渲染
    rendered: list = []
    failures: list = []
    for topic in topics:
        item, failure = render_topic(
            topic, assistants_map, topic_metadata_map, blocks_map, blocks_by_message
        )
        if item is not None:
            rendered.append(item)
        if failure is not None:
            failures.append(failure)

    # 4. 按对话时间从旧到新排序，然后分配条目名
    rendered.sort(key=lambda item: item["epoch"] if item["epoch"] is not None else 0.0)
    used: set = set()
    for item in rendered:
        item["entry_name"] = unique_entry_name(item["entry_name"], used)

    # 5. 定目标路径
    if output is None:
        zip_path = input_path.parent / f"{EXPORT_PREFIX}-{int(datetime.now().timestamp() * 1000)}.zip"
    elif Path(output).suffix.lower() in (".zip", ".md"):
        zip_path = Path(output)
    else:
        zip_path = Path(output)
        zip_path.mkdir(parents=True, exist_ok=True)
        zip_path = zip_path / f"{EXPORT_PREFIX}-{int(datetime.now().timestamp() * 1000)}.zip"

    # 6. 写包（条目时间 = 对话时间；zip 的 DOS 时间戳只有 2 秒精度）
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as archive:
        for item in rendered:
            info = zipfile.ZipInfo(item["entry_name"])
            if item["epoch"] is not None:
                info.date_time = datetime.fromtimestamp(item["epoch"], tz=LOCAL_TZ).timetuple()[:6]
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3  # 3 = Unix（与 Rust 版一致）
            info.external_attr = 0o100644 << 16  # 普通文件 rw-r--r--
            archive.writestr(info, item["markdown"])

        if failures:
            # 失败报告是「刚生成的」，不是某个对话，用当前时间
            report_info = zipfile.ZipInfo("export-failures.md", datetime.now().timetuple()[:6])
            report_info.compress_type = zipfile.ZIP_DEFLATED
            report_info.create_system = 3
            report_info.external_attr = 0o100644 << 16
            archive.writestr(report_info, build_failure_markdown(source, failures))

    print(f"已打包 {len(rendered)} 个对话 → {zip_path}")
    if failures:
        print(f"另有 {len(failures)} 个主题没有任何消息，见包内 export-failures.md")
    return zip_path


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="把 Cherry Studio 备份（JSON 或含 data.json 的 zip）转成契约格式的 Markdown 包"
    )
    parser.add_argument("input_file", nargs="?", default="data.json", help="备份 JSON 或 zip")
    parser.add_argument(
        "-o",
        "--output",
        default=None,
        help="输出目录，或明确的 .zip 路径（省略则与输入同目录）",
    )
    args = parser.parse_args(argv)

    input_path = Path(args.input_file)
    if not input_path.exists():
        print(f"找不到文件: {input_path}")
        return 1

    convert(input_path, args.output, args.input_file)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
