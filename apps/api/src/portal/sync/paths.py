"""Path templating and on-disk conflict resolution for sync downloads.

A sync rule writes each downloaded file under ``destination_path`` at a location derived from its
``path_template``. Templating is pure and tested; conflict resolution touches the filesystem and
is tested against a tmp dir.

Tokens (missing values render empty; unknown tokens are dropped):
  {filename}  full name with extension      {stem}  name without extension
  {ext}       extension without the dot      {basename}  alias for {filename}
  {project}   the rule's project name        {folder}    the rule's source folder name
  {subfolder} source path below the root folder, e.g. ``Dailies/Monday`` (empty at the root)
  {uploader_name}  empty for sync-triggered files (kept for symmetry with upload links)
  {date} YYYY-MM-DD   {year} {month} {day}   {time} 24-hour HHhMM, e.g. 09h30

The source's subfolder tree is mirrored onto the destination by default: the ``{subfolder}`` is
inserted just before the filename. With no template the file goes to ``{subfolder}/{filename}``;
with a template like ``{project}/{date}`` a file in ``Cam-A/Roll1`` yields
``Project/2026-06-10/Cam-A/Roll1/clip.mov``. A template that names ``{subfolder}`` itself controls
its placement (no auto-insert). If a template names no filename token, the basename is appended.
Every path segment is sanitized: separators, NULs, ``..`` and leading dots are stripped so a
template (or a Frame.io name) can never escape the destination root.
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
    # Source subfolder path relative to the rule's root folder, e.g. "Dailies/Monday". Mirrored
    # onto the destination by default (no template); also available as the {subfolder} token.
    subfolder: str = ""
    uploader_name: str = ""
    when: datetime | None = None


def _sanitize_segment(value: str) -> str:
    cleaned = _UNSAFE.sub("_", value).strip().strip(".").strip()
    return cleaned or "_"


def _split_segments(value: str) -> list[str]:
    """Split a relative path string into sanitized, non-empty segments (can't escape root)."""
    return [_sanitize_segment(s) for s in value.replace("\\", "/").split("/") if s.strip()]


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
        "subfolder": ctx.subfolder,
        "uploader_name": ctx.uploader_name,
        "date": when.strftime("%Y-%m-%d"),
        "year": when.strftime("%Y"),
        "month": when.strftime("%m"),
        "day": when.strftime("%d"),
        "time": when.strftime("%Hh%M"),
    }


def render_path_template(template: str | None, ctx: PathContext) -> str:
    """Render a relative POSIX path (always ending in a filename) from a template."""
    basename = _sanitize_segment(ctx.filename)
    sub_segments = _split_segments(ctx.subfolder)
    if not template or not template.strip():
        # No template: mirror the source's subfolder tree under the destination by default.
        return str(PurePosixPath(*sub_segments, basename))

    tokens = _tokens(ctx)
    rendered = _TOKEN.sub(lambda m: tokens.get(m.group(1), ""), template)
    segments = _split_segments(rendered)

    # The subfolder is the file's structure *within* the rule's root, so it belongs immediately
    # before the filename — mirrored by default even with a template. If the template names
    # {subfolder} itself, it's already placed (don't duplicate); the author controls layout.
    insert_sub = sub_segments if "{subfolder}" not in template else []
    if not any(t in template for t in _FILENAME_TOKENS):
        segments = [*segments, *insert_sub, basename]
    elif segments:
        # Filename token present (typically last): tuck the subfolder just before that segment.
        head, last = segments[:-1], segments[-1:]
        segments = [*head, *insert_sub, *last]
    else:
        # Filename token rendered to nothing — fall back to a mirrored basename.
        segments = [*insert_sub, basename]
    return str(PurePosixPath(*segments))


def render_session_prefix(
    template: str | None, *, uploader_name: str = "", when: datetime | None = None
) -> str:
    """Render a template into a sanitized relative *directory* path (no filename appended).

    Used by upload links to place uploads under a per-session subfolder, e.g.
    ``{date}/{uploader_name}`` → ``2026-06-19/Berlin DIT``. Tokens that don't apply to uploads
    (``{filename}``/``{project}``/…) render empty and their segments drop out. Empty/whitespace
    template → ``""``. Every segment is sanitized so a template or uploader name can't escape root.
    """
    if not template or not template.strip():
        return ""
    ctx = PathContext(filename="", uploader_name=uploader_name, when=when)
    tokens = _tokens(ctx)
    rendered = _TOKEN.sub(lambda m: tokens.get(m.group(1), ""), template)
    segments = [_sanitize_segment(s) for s in rendered.replace("\\", "/").split("/") if s.strip()]
    return str(PurePosixPath(*segments)) if segments else ""


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
