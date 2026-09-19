#!/usr/bin/env python3
"""
Analyze high-level JSON skeleton for large chat export files.

Usage:
  python scripts/analyze_json_shape.py <path-to-json> [--sample 500]

Output is a compact JSON report you can paste back without sharing raw content.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Dict, Iterable, Iterator, List, Optional, Tuple


def type_name(v: Any) -> str:
    if v is None:
        return "null"
    if isinstance(v, bool):
        return "bool"
    if isinstance(v, (int, float)):
        return "number"
    if isinstance(v, str):
        return "string"
    if isinstance(v, list):
        return "array"
    if isinstance(v, dict):
        return "object"
    return type(v).__name__


def root_leading_char(path: Path) -> str:
    with path.open("rb") as f:
        while True:
            b = f.read(1)
            if not b:
                return ""
            ch = b.decode("utf-8", errors="ignore")
            if ch == "\ufeff":
                continue
            if not ch.isspace():
                return ch


def try_stream_items(path: Path) -> Optional[Tuple[str, Any, Iterator[Any]]]:
    try:
        import ijson  # type: ignore
    except Exception:
        return None

    lead = root_leading_char(path)
    if lead == "{":
        f = path.open("rb")
        return ("object-with-data-array", f, ijson.items(f, "data.item"))
    if lead == "[":
        f = path.open("rb")
        return ("array", f, ijson.items(f, "item"))

    return None


def get_nested(obj: Dict[str, Any], keys: List[str]) -> Any:
    cur: Any = obj
    for key in keys:
        if not isinstance(cur, dict):
            return None
        cur = cur.get(key)
    return cur


def iter_messages(session: Dict[str, Any]) -> Iterable[Dict[str, Any]]:
    direct = session.get("messages")
    if isinstance(direct, list):
        for m in direct:
            if isinstance(m, dict):
                yield m

    hist_messages = get_nested(session, ["chat", "history", "messages"])
    if isinstance(hist_messages, dict):
        for _, m in hist_messages.items():
            if isinstance(m, dict):
                yield m


def analyze_items(items: Iterable[Any], sample: int) -> Dict[str, Any]:
    item_type_counter: Counter[str] = Counter()
    session_key_counter: Counter[str] = Counter()
    top_level_examples: List[Dict[str, Any]] = []

    message_field_types: Dict[str, Counter[str]] = {
        "content_list": Counter(),
        "childrenIds": Counter(),
        "content": Counter(),
        "reasoning_content": Counter(),
    }
    role_counter: Counter[str] = Counter()

    null_session_items = 0
    invalid_session_like_items = 0
    sampled = 0
    total = 0

    for item in items:
        total += 1
        item_type = type_name(item)
        item_type_counter[item_type] += 1

        if item is None:
            null_session_items += 1
            continue
        if not isinstance(item, dict):
            invalid_session_like_items += 1
            continue

        for k in item.keys():
            session_key_counter[k] += 1

        if len(top_level_examples) < 5:
            top_level_examples.append(
                {
                    "index": total - 1,
                    "keys": sorted(list(item.keys()))[:40],
                }
            )

        for msg in iter_messages(item):
            role = msg.get("role")
            if isinstance(role, str):
                role_counter[role] += 1

            for key in message_field_types:
                message_field_types[key][type_name(msg.get(key))] += 1

        sampled += 1
        if sampled >= sample:
            break

    return {
        "inspected_items": total,
        "sample_limit": sample,
        "item_types": dict(item_type_counter),
        "null_items": null_session_items,
        "non_object_items": invalid_session_like_items,
        "session_top_keys": dict(session_key_counter.most_common(40)),
        "sample_sessions": top_level_examples,
        "message_role_counts": dict(role_counter),
        "message_field_types": {k: dict(v) for k, v in message_field_types.items()},
    }


def analyze_loaded(data: Any, sample: int) -> Dict[str, Any]:
    report: Dict[str, Any] = {
        "root_type": type_name(data),
    }

    if isinstance(data, dict):
        report["root_keys"] = sorted(list(data.keys()))
        if "data" in data:
            report["data_type"] = type_name(data.get("data"))
            if isinstance(data.get("data"), list):
                report["data_len"] = len(data["data"])
                report["data_analysis"] = analyze_items(data["data"], sample)
    elif isinstance(data, list):
        report["root_len"] = len(data)
        report["data_analysis"] = analyze_items(data, sample)

    return report


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("json_path", type=Path)
    parser.add_argument("--sample", type=int, default=500)
    args = parser.parse_args()

    path: Path = args.json_path
    if not path.exists():
        raise SystemExit(f"file not found: {path}")

    stream_info = try_stream_items(path)
    if stream_info is not None:
        root_hint, stream_file, stream = stream_info
        try:
            analysis = analyze_items(stream, args.sample)
        finally:
            stream_file.close()

        report = {
            "stream_mode": "ijson",
            "root_hint": root_hint,
            "data_analysis": analysis,
        }
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return

    # Fallback: full load with stdlib json.
    with path.open("r", encoding="utf-8") as f:
        data = json.load(f)
    report = {
        "stream_mode": "stdlib-json-load",
        **analyze_loaded(data, args.sample),
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
