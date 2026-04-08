from __future__ import annotations

import json

from backend.src.harness.runner import _build_summary_text
from backend.src.routes.runs import _normalize_contract, _normalize_qa_report


def test_normalize_contract_supports_structured_contract_payload():
    row = {
        "contract_id": "c1",
        "sprint": 3,
        "status": "accepted",
        "contract_json": json.dumps(
            {
                "objective": "Finish run detail page",
                "scope_in": ["Render approvals", "Render QA summaries"],
                "done_definition": "Run detail page exposes approvals and QA.",
            }
        ),
        "created_at": "2026-04-04T00:00:00+00:00",
    }

    payload = _normalize_contract(row)

    assert payload["objective"] == "Finish run detail page"
    assert payload["scope_in"] == ["Render approvals", "Render QA summaries"]
    assert payload["done_definition"] == "Run detail page exposes approvals and QA."


def test_normalize_qa_report_backfills_scores_for_legacy_payload():
    row = {
        "qa_id": "q1",
        "sprint": 1,
        "pass": 0,
        "report_json": json.dumps(
            {
                "criteria_results": [
                    {"criterion": "UI renders", "pass": True, "notes": "looks good"},
                    {"criterion": "Approval controls work", "pass": False, "notes": "gate is never visible"},
                ]
            }
        ),
        "created_at": "2026-04-04T00:00:00+00:00",
    }

    payload = _normalize_qa_report(row)
    scores = json.loads(payload["scores_json"])
    issues = json.loads(payload["blocking_issues_json"])

    assert payload["result"] == "fail"
    assert scores["functionality"] == 0.5
    assert issues[0]["title"] == "Approval controls work"


def test_build_summary_text_includes_contracts_and_qa_results():
    run_row = {"prompt": "Improve the harness", "state": "completed"}
    contracts = [{"sprint": 1, "done_definition": "Stabilize approvals"}]
    reports = [{"sprint": 1, "result": "fail", "blocking_issues": [{"title": "Gate mismatch"}]}]

    summary = _build_summary_text(run_row, contracts, reports)

    assert "Improve the harness" in summary
    assert "Stabilize approvals" in summary
    assert "Gate mismatch" in summary
