from datetime import datetime
from pathlib import Path

from portal.sync.paths import (
    PathContext,
    render_path_template,
    render_session_prefix,
    resolve_conflict,
    resolve_destination,
)

WHEN = datetime(2026, 6, 10, 14, 30)


def test_render_session_prefix_empty() -> None:
    assert render_session_prefix(None, uploader_name="X") == ""
    assert render_session_prefix("   ", uploader_name="X") == ""


def test_render_session_prefix_tokens() -> None:
    assert (
        render_session_prefix("{date}/{uploader_name}", uploader_name="DIT", when=WHEN)
        == "2026-06-10/DIT"
    )
    assert render_session_prefix("{year}/{month}/{day}", when=WHEN) == "2026/06/10"


def test_render_session_prefix_drops_inapplicable_tokens() -> None:
    # No filename is appended (directory-only); filename tokens render empty → nothing.
    assert render_session_prefix("{filename}", uploader_name="X") == ""


def test_render_session_prefix_cannot_escape_root() -> None:
    out = render_session_prefix("{uploader_name}", uploader_name="../../etc")
    assert ".." not in out


def test_no_template_uses_basename() -> None:
    ctx = PathContext(filename="Shot_01.mov")
    assert render_path_template(None, ctx) == "Shot_01.mov"
    assert render_path_template("   ", ctx) == "Shot_01.mov"


def test_template_tokens() -> None:
    ctx = PathContext(filename="Shot_01.mov", project="Acme", folder="Dailies", when=WHEN)
    assert render_path_template("{project}/{date}/{filename}", ctx) == "Acme/2026-06-10/Shot_01.mov"
    assert render_path_template("{folder}/{stem}.{ext}", ctx) == "Dailies/Shot_01.mov"
    assert render_path_template("{year}/{month}/{day}/{filename}", ctx) == "2026/06/10/Shot_01.mov"


def test_no_template_mirrors_subfolder() -> None:
    ctx = PathContext(filename="clip.mov", subfolder="Dailies/Monday")
    assert render_path_template(None, ctx) == "Dailies/Monday/clip.mov"


def test_no_template_empty_subfolder_unchanged() -> None:
    ctx = PathContext(filename="clip.mov", subfolder="")
    assert render_path_template(None, ctx) == "clip.mov"


def test_subfolder_token_in_template() -> None:
    ctx = PathContext(filename="clip.mov", project="Acme", subfolder="A/B", when=WHEN)
    assert render_path_template("{project}/{subfolder}/{filename}", ctx) == "Acme/A/B/clip.mov"


def test_template_without_filename_token_keeps_subfolder() -> None:
    # The user's case: a {project}/{date} template should still mirror the source subfolders.
    ctx = PathContext(filename="clip.mov", project="Acme", subfolder="Cam-A/Roll1", when=WHEN)
    assert (
        render_path_template("{project}/{date}", ctx)
        == "Acme/2026-06-10/Cam-A/Roll1/clip.mov"
    )


def test_template_with_filename_token_inserts_subfolder_before_filename() -> None:
    ctx = PathContext(filename="clip.mov", project="Acme", subfolder="Cam-A", when=WHEN)
    assert render_path_template("{project}/{filename}", ctx) == "Acme/Cam-A/clip.mov"


def test_subfolder_cannot_escape_root() -> None:
    ctx = PathContext(filename="clip.mov", subfolder="../../etc")
    p = resolve_destination("/data/out", None, ctx)
    assert ".." not in p.parts
    assert str(p).startswith("/data/out/")
    assert p.name == "clip.mov"


def test_template_without_filename_token_appends_basename() -> None:
    ctx = PathContext(filename="clip.mov", project="Proj", when=WHEN)
    assert render_path_template("{project}/{date}", ctx) == "Proj/2026-06-10/clip.mov"


def test_unknown_tokens_dropped_empty_values_collapse() -> None:
    ctx = PathContext(filename="clip.mov")  # no project/folder
    assert render_path_template("{project}/{nope}/{filename}", ctx) == "clip.mov"


def test_traversal_is_sanitized() -> None:
    # A malicious filename collapses to one safe segment (separators neutralized) and can't escape.
    ctx = PathContext(filename="../../etc/passwd")
    p = resolve_destination("/data/out", None, ctx)
    assert ".." not in p.parts
    assert str(p).startswith("/data/out/")

    # A malicious template token can't introduce a parent ref either.
    ctx2 = PathContext(filename="x.mov", project="../../evil")
    p2 = resolve_destination("/data/out", "{project}/{filename}", ctx2)
    assert ".." not in p2.parts
    assert str(p2).startswith("/data/out/")
    assert p2.name == "x.mov"


def test_resolve_destination_joins_root() -> None:
    ctx = PathContext(filename="a.mov", project="P", when=WHEN)
    p = resolve_destination("/data/out", "{project}/{filename}", ctx)
    assert p == Path("/data/out/P/a.mov")


def test_conflict_skip(tmp_path: Path) -> None:
    f = tmp_path / "a.mov"
    f.write_bytes(b"x")
    assert resolve_conflict(f, "skip") is None
    assert resolve_conflict(tmp_path / "b.mov", "skip") == tmp_path / "b.mov"


def test_conflict_overwrite(tmp_path: Path) -> None:
    f = tmp_path / "a.mov"
    f.write_bytes(b"x")
    assert resolve_conflict(f, "overwrite") == f


def test_conflict_rename_suffix(tmp_path: Path) -> None:
    f = tmp_path / "a.mov"
    f.write_bytes(b"x")
    assert resolve_conflict(f, "rename_suffix") == tmp_path / "a_1.mov"
    (tmp_path / "a_1.mov").write_bytes(b"x")
    assert resolve_conflict(f, "rename_suffix") == tmp_path / "a_2.mov"
