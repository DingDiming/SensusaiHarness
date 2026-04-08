from __future__ import annotations

import json

import pytest

from backend.src.harness.artifacts import (
    write_checkpoint_metadata,
    write_handoff,
    write_product_spec,
    write_qa_report,
    write_run_summary,
    write_sprint_contract,
)
from backend.src.harness.workspace import artifact_path, ensure_workspace


def test_artifact_path_blocks_traversal(tmp_path):
    ensure_workspace(str(tmp_path))

    with pytest.raises(ValueError):
        artifact_path(str(tmp_path), "../outside.txt")


def test_artifact_writers_create_expected_files(tmp_path):
    ensure_workspace(str(tmp_path))

    spec_path, _, _ = write_product_spec(str(tmp_path), "# Product Spec")
    contract_path, _, _ = write_sprint_contract(str(tmp_path), 2, {"objective": "Ship dashboard"})
    qa_path, _, _ = write_qa_report(str(tmp_path), 2, {"result": "pass"})
    handoff_path, _, _ = write_handoff(str(tmp_path), 2, "# Handoff")
    checkpoint_path, _, _ = write_checkpoint_metadata(str(tmp_path), 2, {"name": "sprint-02"})
    summary_path, _, _ = write_run_summary(str(tmp_path), "# Summary")

    assert (tmp_path / spec_path).read_text(encoding="utf-8") == "# Product Spec"
    assert (tmp_path / contract_path).exists()
    assert json.loads((tmp_path / contract_path).read_text(encoding="utf-8"))["objective"] == "Ship dashboard"
    assert json.loads((tmp_path / qa_path).read_text(encoding="utf-8"))["result"] == "pass"
    assert (tmp_path / handoff_path).read_text(encoding="utf-8") == "# Handoff"
    assert json.loads((tmp_path / checkpoint_path).read_text(encoding="utf-8"))["name"] == "sprint-02"
    assert (tmp_path / summary_path).read_text(encoding="utf-8") == "# Summary"
