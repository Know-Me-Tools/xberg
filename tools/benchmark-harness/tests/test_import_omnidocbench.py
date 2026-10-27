"""Keep independent layout annotations available alongside text projections."""

import copy
import hashlib
import importlib.util
import json
from pathlib import Path

import pytest

SCRIPT = Path(__file__).parents[1] / "scripts" / "import_omnidocbench.py"
SPEC = importlib.util.spec_from_file_location("import_omnidocbench_under_test", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
ImportSource = tuple[Path, Path, Path, list[dict]]

IMPORTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(IMPORTER)


@pytest.fixture
def source(tmp_path: Path) -> ImportSource:
    pdfs = tmp_path / "pdfs"
    fixtures = tmp_path / "fixtures"
    truth = tmp_path / "truth"
    for directory in (pdfs, fixtures, truth):
        directory.mkdir()
    (pdfs / "document.pdf").write_bytes(b"%PDF-1.7\ncontrolled importer fixture\n")
    pages = [
        {
            "page_info": {"page_no": 1, "width": 800, "height": 1000, "image_path": "document_p1.jpg"},
            "layout_dets": [
                {
                    "anno_id": 9,
                    "category_type": "title",
                    "order": 2,
                    "poly": [10, 20, 100, 20, 100, 50, 10, 50],
                    "text": "Independent title",
                    "line_with_spans": [{"category_type": "equation_inline", "latex": "x^2", "order": 0}],
                },
                {
                    "anno_id": 4,
                    "category_type": "figure",
                    "order": 1,
                    "poly": [20, 80, 300, 80, 300, 250, 20, 250],
                    "ignore": True,
                },
            ],
            "extra": {"relation": [{"source_anno_id": 9, "target_anno_id": 4, "relation": "caption"}]},
        },
        {"page_info": {"page_no": 2, "image_path": "document_p2.jpg"}, "layout_dets": []},
    ]
    return pdfs, fixtures, truth, pages


def run_import(source: ImportSource) -> tuple[str, dict]:
    pdfs, fixtures, truth, pages = source
    status = IMPORTER.import_single_document("document", pages, pdfs, fixtures, truth)
    fixture = json.loads((fixtures / "omnidoc_document.json").read_text())
    return status, fixture


def load_layout(source: ImportSource, fixture: dict) -> dict:
    _, fixtures, _, _ = source
    metadata = fixture["metadata"]["layout_ground_truth"]
    path = (fixtures / metadata["file"]).resolve()
    payload = path.read_bytes()
    assert hashlib.sha256(payload).hexdigest() == metadata["sha256"]
    return json.loads(payload)


def test_import_preserves_all_original_pages_annotations_and_relations(source: ImportSource) -> None:
    before = copy.deepcopy(source[3])
    status, fixture = run_import(source)
    layout = load_layout(source, fixture)
    assert status == IMPORTER.IMPORT_STATUS_CREATED
    assert layout["pages"] == before
    assert source[3] == before
    assert layout["page_count"] == 2
    assert layout["annotation_count"] == 2
    assert layout["schema_version"] == 1
    assert layout["source"] == "omnidocbench"
    assert fixture["metadata"]["page_count"] == 2
    assert (source[2] / "omnidoc_document.md").read_text() == "## Independent title"


def test_layout_identity_binds_exact_pdf_and_source_annotations(source: ImportSource) -> None:
    _, fixture = run_import(source)
    layout = load_layout(source, fixture)
    pdf = (source[0] / "document.pdf").read_bytes()
    assert layout["document"] == {"sha256": hashlib.sha256(pdf).hexdigest(), "size_bytes": len(pdf)}
    canonical = json.dumps(source[3], sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
    assert layout["annotations_sha256"] == hashlib.sha256(canonical.encode()).hexdigest()


def test_existing_text_fixture_is_backfilled_without_rewriting_text_ground_truth(source: ImportSource) -> None:
    _, fixture = run_import(source)
    layout_path = (source[1] / fixture["metadata"]["layout_ground_truth"]["file"]).resolve()
    layout_path.unlink()
    fixture["metadata"].pop("layout_ground_truth")
    fixture["metadata"]["reviewed"] = "keep"
    (source[1] / "omnidoc_document.json").write_text(json.dumps(fixture))
    text_path = source[2] / "omnidoc_document.md"
    text_path.write_text("Independently corrected text")
    status, updated = run_import(source)
    assert status == IMPORTER.IMPORT_STATUS_EXISTS
    assert text_path.read_text() == "Independently corrected text"
    assert updated["metadata"]["reviewed"] == "keep"
    assert load_layout(source, updated)["pages"] == source[3]


def test_reimport_is_byte_identical(source: ImportSource) -> None:
    run_import(source)
    files = [*source[1].iterdir(), *source[2].iterdir()]
    before = {path: path.read_bytes() for path in files}
    run_import(source)
    assert {path: path.read_bytes() for path in files} == before


@pytest.mark.parametrize("mutation", ["pdf", "annotations", "sidecar"])
def test_reimport_rejects_changed_identity_instead_of_silently_overwriting(source: ImportSource, mutation: str) -> None:
    _, fixture = run_import(source)
    if mutation == "pdf":
        (source[0] / "document.pdf").write_bytes(b"different PDF")
    elif mutation == "annotations":
        source[3][0]["layout_dets"][0]["order"] = 0
    else:
        path = (source[1] / fixture["metadata"]["layout_ground_truth"]["file"]).resolve()
        path.write_text("{}")
    with pytest.raises(ValueError, match="layout ground truth identity mismatch"):
        run_import(source)


def test_invalid_nonfinite_annotations_fail_before_any_output(source: ImportSource) -> None:
    source[3][0]["layout_dets"][0]["poly"][0] = float("nan")
    with pytest.raises(ValueError, match="Out of range float values"):
        run_import(source)
    assert list(source[1].iterdir()) == []
    assert list(source[2].iterdir()) == []


def test_missing_pdf_does_not_emit_annotation_or_fixture_output(source: ImportSource) -> None:
    (source[0] / "document.pdf").unlink()
    assert IMPORTER.import_single_document("document", source[3], *source[:3]) == IMPORTER.IMPORT_STATUS_NO_PDF
    assert list(source[1].iterdir()) == []
    assert list(source[2].iterdir()) == []


def test_visual_only_document_retains_existing_empty_content_policy(source: ImportSource) -> None:
    source[3][0]["layout_dets"] = [source[3][0]["layout_dets"][1]]
    assert IMPORTER.import_single_document("document", source[3], *source[:3]) == IMPORTER.IMPORT_STATUS_EMPTY
    assert list(source[1].iterdir()) == []
    assert list(source[2].iterdir()) == []


def test_conflicting_orphan_sidecar_does_not_overwrite_text(source: ImportSource) -> None:
    sidecar = source[2] / "omnidoc_document.layout.json"
    sidecar.write_text("frozen older annotations")
    text = source[2] / "omnidoc_document.md"
    text.write_text("Reviewed text")
    with pytest.raises(ValueError, match="layout ground truth identity mismatch"):
        run_import(source)
    assert text.read_text() == "Reviewed text"
    assert sidecar.read_text() == "frozen older annotations"
    assert list(source[1].iterdir()) == []
