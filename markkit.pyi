"""Extract clean Markdown from any document format, safely.

Markkit converts HTML, PDF, DOCX, PPTX, spreadsheets (XLSX/XLSB/XLS/ODS), ODT,
EPUB, email (EML), Jupyter notebooks, CSV/TSV, Markdown and plain text into
well-structured Markdown for LLMs and agents. Every input is treated as
untrusted: nothing is executed and every resource an input can drive is
bounded by :class:`Limits`.

Errors: resource-limit breaches, unsupported formats, parse failures and
blocked URLs raise ``RuntimeError`` with a message naming the cause; invalid
arguments raise ``ValueError``.
"""

from typing import Any, Awaitable, Literal, Optional, Sequence, TypedDict

__all__ = [
    "Limits",
    "Extraction",
    "ExtractionStats",
    "Metadata",
    "Warning",
    "extract_document_sync",
    "extract_html",
    "extract_html_sync",
    "extract_html_sync_with_options",
    "extract_url",
    "extract_bytes",
]

class Limits:
    """Resource limits applied to one call. Every argument is optional and
    defaults to the library default; pass the object as ``limits=``.

    Breaches raise ``RuntimeError`` naming the limit, e.g.
    ``"Resource limit exceeded: output exceeds max_output_size 256"``.
    Spreadsheet rows/columns and PDF pages are truncated with a warning instead.
    """

    max_input_size: int
    """Input bytes, and the body of a fetched URL. Default 50 MiB."""
    max_output_size: int
    """Markdown output bytes. Default 128 MiB."""
    timeout: float
    """Seconds for fetch + parse + render of one call. Default 60."""
    max_nesting_depth: int
    """Element nesting; deeper HTML is flattened. Default 256, hard max 512."""
    max_nodes: int
    """Parsed HTML DOM / XML nodes and retained PDF geometry. Default 5 000 000."""
    max_archive_entries: int
    """ZIP entries in DOCX/PPTX/XLSX/ODT/EPUB containers. Default 10 000."""
    max_archive_entry_size: int
    """Decompressed bytes per ZIP entry. Default 64 MiB."""
    max_archive_total_size: int
    """Decompressed bytes per ZIP container. Default 256 MiB."""
    max_sheet_rows: int
    """Rows rendered per sheet (truncated with a warning). Default 10 000."""
    max_sheet_cols: int
    """Columns rendered per sheet (truncated with a warning). Default 256."""
    max_spreadsheet_cells: int
    """XLS/ODS grid cells checked before parsing. Default 4 000 000."""
    max_redirects: int
    """Redirects followed by :func:`extract_url`. Default 5."""
    max_pdf_pages: int
    """PDF pages rendered (truncated with a warning). Default 10 000."""
    max_pdf_stream_size: int
    """Decoded bytes per PDF stream. Default 64 MiB."""
    max_pdf_decoded_size: int
    """Decoded PDF bytes in total, also bounds layout geometry. Default 256 MiB."""

    def __init__(
        self,
        *,
        max_input_size: Optional[int] = None,
        max_output_size: Optional[int] = None,
        timeout: Optional[float] = None,
        max_nesting_depth: Optional[int] = None,
        max_nodes: Optional[int] = None,
        max_archive_entries: Optional[int] = None,
        max_archive_entry_size: Optional[int] = None,
        max_archive_total_size: Optional[int] = None,
        max_sheet_rows: Optional[int] = None,
        max_sheet_cols: Optional[int] = None,
        max_spreadsheet_cells: Optional[int] = None,
        max_redirects: Optional[int] = None,
        max_pdf_pages: Optional[int] = None,
        max_pdf_stream_size: Optional[int] = None,
        max_pdf_decoded_size: Optional[int] = None,
    ) -> None: ...

class Warning(TypedDict):
    """A non-fatal extraction issue."""

    code: str
    """Stable machine code, e.g. ``"truncated_sheet"``, ``"hidden_sheet_skipped"``."""
    detail: str
    """Human-readable description."""

class ExtractionStats(TypedDict, total=False):
    """Counts for the document; a key is ``None`` when it does not apply."""

    pages: Optional[int]
    sheets: Optional[int]
    rows: Optional[int]
    paragraphs: Optional[int]
    tables: Optional[int]
    cells: Optional[int]
    attachments: Optional[int]
    chapters: Optional[int]

class Metadata(TypedDict, total=False):
    """HTML page metadata from ``<meta>``, Open Graph and JSON-LD."""

    description: Optional[str]
    author: Optional[str]
    published: Optional[str]
    language: Optional[str]
    canonical_url: Optional[str]
    site_name: Optional[str]
    image: Optional[str]

class Extraction(TypedDict, total=False):
    """Result of :func:`extract_document_sync`."""

    markdown: str
    """The extracted Markdown."""
    title: Optional[str]
    """Document title when the source declares one."""
    format: str
    """Detected format: ``html``, ``pdf``, ``docx``, ``pptx``, ``xlsx``,
    ``xlsb``, ``xls``, ``ods``, ``odt``, ``epub``, ``eml``, ``ipynb``,
    ``csv``, ``tsv``, ``markdown`` or ``text``."""
    warnings: list[Warning]
    stats: ExtractionStats
    metadata: Metadata
    layout: dict[str, Any]
    """PDF geometry, only with ``pdf_layout=True`` on PDF input:
    ``{"version": 1, "pages": [{"page", "width", "height", "rotation",
    "spans", "blocks"}]}``. Coordinates are points from the top-left."""

def extract_document_sync(
    data: bytes,
    *,
    file_name: Optional[str] = None,
    content_type: Optional[str] = None,
    pdf_layout: bool = False,
    pdf_remove_repeated_headers: bool = False,
    pdf_ocr: bool = False,
    pdf_ocr_language: str = "eng",
    pdf_ocr_backend: Literal["tesseract", "ocrs"] = "tesseract",
    pdf_ocr_detection_model: Optional[str] = None,
    pdf_ocr_recognition_model: Optional[str] = None,
    limits: Optional[Limits] = None,
) -> Extraction:
    """Convert document bytes of any supported format to Markdown plus
    title, format, warnings, stats and metadata.

    The format is detected from ``content_type``, then the extension of
    ``file_name``, then magic bytes. ``pdf_layout`` adds PDF geometry,
    ``pdf_remove_repeated_headers`` drops repeated page headers/footers,
    ``pdf_ocr`` recognizes textless PDF pages (requires Tesseract and
    language data, e.g. ``pdf_ocr_language="eng+deu"``).

    >>> r = extract_document_sync(b"a,b\\n1,2\\n", file_name="t.csv")
    >>> r["format"], r["markdown"].splitlines()[0]
    ('csv', '| a | b |')
    """

def extract_html_sync(html: str, *, limits: Optional[Limits] = None) -> str:
    """Convert an HTML string to Markdown. Navigation, headers, footers,
    scripts, styles and forms are stripped.

    >>> extract_html_sync("<h1>Hello</h1>")
    '# Hello\\n'
    """

def extract_html_sync_with_options(
    html: str,
    *,
    include_selectors: Optional[Sequence[str]] = None,
    exclude_selectors: Optional[Sequence[str]] = None,
    max_input_size: Optional[int] = None,
    base_url: Optional[str] = None,
    main_content: bool = False,
    limits: Optional[Limits] = None,
) -> str:
    """Convert HTML with CSS selector filtering and link resolution.

    ``include_selectors`` keeps only matching elements; ``exclude_selectors``
    drops matches (added to the defaults); ``base_url`` resolves relative
    links and images; ``main_content`` keeps only the detected article body;
    ``max_input_size`` overrides ``limits.max_input_size``.
    """

def extract_html(html: str, *, limits: Optional[Limits] = None) -> Awaitable[str]:
    """Async :func:`extract_html_sync`: ``md = await markkit.extract_html(html)``."""

def extract_url(url: str, *, limits: Optional[Limits] = None) -> Awaitable[str]:
    """Fetch an http(s) URL and convert the response (any supported format).

    Loopback, private, link-local and cloud-metadata addresses are refused,
    also after DNS resolution and on every redirect. The body is bounded by
    ``limits.max_input_size`` and the call by ``limits.timeout``.
    """

def extract_bytes(
    data: bytes, content_type: Optional[str] = None, *, limits: Optional[Limits] = None
) -> Awaitable[str]:
    """Async conversion of bytes to Markdown only; the format is detected
    from ``content_type`` and the bytes. Use :func:`extract_document_sync`
    for title, warnings and stats."""
