"""Keep independent layout annotations available alongside text projections."""

import copy
import hashlib
import importlib.util
import json
import subprocess
import sys
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


def cli_source(source: ImportSource, tmp_path: Path) -> tuple[Path, Path]:
    dataset = tmp_path / "dataset"
    originals = dataset / "ori_pdfs"
    originals.mkdir(parents=True)
    (originals / "document.pdf").write_bytes((source[0] / "document.pdf").read_bytes())
    (dataset / "OmniDocBench.json").write_text(json.dumps(source[3]))
    return dataset, tmp_path / "repository"


def run_cli(dataset: Path, repository: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), str(dataset), str(repository), *arguments],
        capture_output=True,
        text=True,
        check=False,
    )


@pytest.mark.parametrize("missing", ["directory", "empty_directory", "unmatched_pdf", "empty_content"])
def test_cli_rejects_zero_validated_documents(source: ImportSource, tmp_path: Path, missing: str) -> None:
    dataset, repository = cli_source(source, tmp_path)
    if missing == "empty_content":
        pages = copy.deepcopy(source[3])
        pages[0]["layout_dets"] = [pages[0]["layout_dets"][1]]
        (dataset / "OmniDocBench.json").write_text(json.dumps(pages))
    else:
        (dataset / "ori_pdfs" / "document.pdf").unlink()
        if missing == "directory":
            (dataset / "ori_pdfs").rmdir()
        elif missing == "unmatched_pdf":
            (dataset / "ori_pdfs" / "unrelated.pdf").write_bytes(b"%PDF-1.7\nunrelated input\n")
    result = run_cli(dataset, repository)
    assert result.returncode != 0
    assert "no documents and pages were validated" in result.stderr
    assert "Done:" not in result.stderr


def test_cli_rejects_empty_annotation_set(source: ImportSource, tmp_path: Path) -> None:
    dataset, repository = cli_source(source, tmp_path)
    (dataset / "OmniDocBench.json").write_text("[]")
    result = run_cli(dataset, repository)
    assert result.returncode != 0
    assert "no documents and pages were validated" in result.stderr


@pytest.mark.parametrize(
    ("arguments", "reason"),
    [
        (("--expected-documents", "2", "--expected-pages", "3"), "expected 2 validated documents, found 1"),
        (("--expected-documents", "1", "--expected-pages", "3"), "expected 3 validated pages, found 2"),
    ],
)
def test_cli_rejects_partial_expected_counts(
    source: ImportSource, tmp_path: Path, arguments: tuple[str, ...], reason: str
) -> None:
    dataset, repository = cli_source(source, tmp_path)
    missing = copy.deepcopy(source[3][0])
    missing["page_info"]["image_path"] = "missing_p1.jpg"
    (dataset / "OmniDocBench.json").write_text(json.dumps([*source[3], missing]))
    result = run_cli(dataset, repository, *arguments)
    assert result.returncode != 0
    assert reason in result.stderr
    assert "Done:" not in result.stderr


@pytest.mark.parametrize("option", ["--expected-documents", "--expected-pages"])
@pytest.mark.parametrize("count", ["0", "-1"])
def test_cli_rejects_nonpositive_expected_counts(source: ImportSource, tmp_path: Path, option: str, count: str) -> None:
    dataset, repository = cli_source(source, tmp_path)
    result = run_cli(dataset, repository, option, count)
    assert result.returncode != 0
    assert "must be a positive integer" in result.stderr
    assert not repository.exists()


@pytest.mark.parametrize("first_page", [0, 1])
def test_cli_counts_verified_existing_fixtures_without_new_writes(
    source: ImportSource, tmp_path: Path, first_page: int
) -> None:
    dataset, repository = cli_source(source, tmp_path)
    pages = copy.deepcopy(source[3])
    for index, page in enumerate(pages, start=first_page):
        page["page_info"]["page_no"] = index
        page["page_info"]["image_path"] = f"document_p{index}.jpg"
    (dataset / "OmniDocBench.json").write_text(json.dumps(pages))
    arguments = ("--expected-documents", "1", "--expected-pages", "2")
    first = run_cli(dataset, repository, *arguments)
    assert first.returncode == 0, first.stderr
    outputs = [path for path in repository.rglob("*") if path.is_file()]
    assert len(outputs) == 4
    before = {path: path.read_bytes() for path in outputs}
    repeated = run_cli(dataset, repository, *arguments)
    assert repeated.returncode == 0, repeated.stderr
    assert "Created: 0" in repeated.stderr
    assert "Existing text fixtures (layout verified/backfilled): 1" in repeated.stderr
    assert {path: path.read_bytes() for path in outputs} == before


def test_cli_does_not_count_unrelated_existing_fixtures(source: ImportSource, tmp_path: Path) -> None:
    dataset, repository = cli_source(source, tmp_path)
    assert run_cli(dataset, repository).returncode == 0
    pages = copy.deepcopy(source[3])
    for index, page in enumerate(pages, start=1):
        page["page_info"]["image_path"] = f"unavailable_p{index}.jpg"
    (dataset / "OmniDocBench.json").write_text(json.dumps(pages))
    result = run_cli(dataset, repository)
    assert result.returncode != 0
    assert "no documents and pages were validated" in result.stderr


@pytest.mark.parametrize("invalid", ["empty_image", "duplicate_page"])
def test_cli_rejects_invalid_source_page_ids(source: ImportSource, tmp_path: Path, invalid: str) -> None:
    dataset, repository = cli_source(source, tmp_path)
    pages = copy.deepcopy(source[3])
    if invalid == "empty_image":
        pages[0]["page_info"]["image_path"] = ""
    else:
        pages.append(copy.deepcopy(pages[0]))
    (dataset / "OmniDocBench.json").write_text(json.dumps(pages))
    result = run_cli(dataset, repository)
    assert result.returncode != 0
    assert "invalid source page identity" in result.stderr
    assert not repository.exists()
