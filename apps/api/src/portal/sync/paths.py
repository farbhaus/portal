"""Path templating and on-disk conflict resolution for sync downloads.

A sync rule writes each downloaded file under ``destination_path`` at a location derived from its
``path_template``. Templating is pure and tested; conflict resolution touches the filesystem and
is tested against a tmp dir.

Tokens (missing values render empty; unknown tokens are dropped):
  {filename}  full name with extension      {stem}  name without extension
  {ext}       extension without the dot      {basename}  alias for {filename}
  {project}   the rule's project name        {folder}    the rule's source folder name
  {uploader_name}  empty for sync-triggered files (kept for symmetry with upload links)
  {date} YYYY-MM-DD   {year} {month} {day}

If the template names no filename token, the basename is appended — so ``{project}/{date}`` yields
``Project/2026-06-10/clip.mov``. Every path segment is sanitized: separators, NULs, ``..`` and
leading dots are stripped so a template (or a Frame.io name) can never escape the destination root.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path, PurePosixPath

_TOKEN = re.compile(r"\{(\w+)\}")
_UNSAFE = re.compile(r"[\x00-\x1f/\\]")
_FILENAME_TOKENS = ("filename", "basename", "stem")


@dataclass(frozen=True)
class PathContext:
    filename: str  # full name including extension
    project: str = ""
    folder: str = ""
    uploader_name: str = ""
    when: datetime | None = None


def _sanitize_segment(value: str) -> str:
    cleaned = _UNSAFE.sub("_", value).strip().strip(".").strip()
    return cleaned or "_"


def _tokens(ctx: PathContext) -> dict[str, str]:
    name = ctx.filename
    when = ctx.when or datetime.now()
    return {
        "filename": name,
        "basename": name,
        "stem": Path(name).stem,
        "ext": Path(name).suffix.lstrip("."),
        "project": ctx.project,
        "folder": ctx.folder,
        "uploader_name": ctx.uploader_name,
        "date": when.strftime("%Y-%m-%d"),
        "year": when.strftime("%Y"),
        "month": when.strftime("%m"),
        "day": when.strftime("%d"),
    }


def render_path_template(template: str | None, ctx: PathContext) -> str:
    """Render a relative POSIX path (always ending in a filename) from a template."""
    basename = _sanitize_segment(ctx.filename)
    if not template or not template.strip():
        return basename

    tokens = _tokens(ctx)
    rendered = _TOKEN.sub(lambda m: tokens.get(m.group(1), ""), template)

    segments = [_sanitize_segment(s) for s in rendered.replace("\\", "/").split("/") if s.strip()]
    if not any(t in template for t in _FILENAME_TOKENS):
        segments.append(basename)
    if not segments:
        segments = [basename]
    return str(PurePosixPath(*segments))


def resolve_destination(root: str, template: str | None, ctx: PathContext) -> Path:
    """Absolute destination path under ``root`` for a file, before conflict resolution."""
    relative = render_path_template(template, ctx)
    return Path(root) / relative


def resolve_conflict(path: Path, policy: str) -> Path | None:
    """Apply the conflict policy for an existing file. Returns the path to write, or None to skip.

    - ``skip``: None if the file already exists.
    - ``overwrite``: the same path (caller replaces it).
    - ``rename_suffix``: ``name_1.ext``, ``name_2.ext``, ... until a free name is found.
    """
    if not path.exists():
        return path
    if policy == "overwrite":
        return path
    if policy == "skip":
        return None
    # rename_suffix (default)
    stem, suffix = path.stem, path.suffix
    i = 1
    while True:
        candidate = path.with_name(f"{stem}_{i}{suffix}")
        if not candidate.exists():
            return candidate
        i += 1
