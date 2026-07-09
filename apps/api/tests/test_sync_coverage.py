"""Unit tests for matching sync rules to destinations (portal.services.sync_coverage)."""

from portal.db.models import SyncRule
from portal.services.sync_coverage import rules_covering

DEST = {
    "type": "frameio",
    "account_id": "a1",
    "workspace_id": "w1",
    "project_id": "p1",
    "folder_id": "leaf",
    "path": [
        {"id": "root", "name": "Proj (root)"},
        {"id": "mid", "name": "Mid"},
        {"id": "leaf", "name": "Leaf"},
    ],
}


def _rule(**src: object) -> SyncRule:
    return SyncRule(
        name="r",
        source_config={"type": "frameio", "account_id": "a1", **src},
        destination_path="/data/out",
        conflict_policy="rename_suffix",
        enabled=True,
    )


def test_exact_match() -> None:
    rule = _rule(folder_id="leaf")
    assert rules_covering(DEST, [rule]) == [(rule, True)]


def test_recursive_ancestor_matches() -> None:
    for ancestor in ("root", "mid"):
        rule = _rule(folder_id=ancestor, recursive=True)
        assert rules_covering(DEST, [rule]) == [(rule, False)]


def test_non_recursive_ancestor_does_not_match() -> None:
    rule = _rule(folder_id="mid", recursive=False)
    assert rules_covering(DEST, [rule]) == []


def test_unrelated_folder_does_not_match() -> None:
    rule = _rule(folder_id="elsewhere", recursive=True)
    assert rules_covering(DEST, [rule]) == []


def test_cross_account_never_matches() -> None:
    exact = _rule(folder_id="leaf")
    exact.source_config["account_id"] = "other"
    ancestor = _rule(folder_id="mid", recursive=True)
    ancestor.source_config["account_id"] = "other"
    assert rules_covering(DEST, [exact, ancestor]) == []


def test_missing_path_degrades_to_exact_only() -> None:
    dest = {k: v for k, v in DEST.items() if k != "path"}
    exact = _rule(folder_id="leaf")
    ancestor = _rule(folder_id="mid", recursive=True)
    assert rules_covering(dest, [exact, ancestor]) == [(exact, True)]
