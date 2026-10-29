use std::time::Duration;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

/// Resource limits (mirrors `markkit::Limits`). Every argument is optional and
/// defaults to the library default; violations raise `RuntimeError` naming
/// the limit.
///
/// Keyword arguments: max_input_size, max_output_size, timeout (seconds,
/// float), max_nesting_depth, max_nodes, max_archive_entries,
/// max_archive_entry_size, max_archive_total_size, max_sheet_rows,
/// max_sheet_cols, max_spreadsheet_cells, max_redirects, max_pdf_pages,
/// max_pdf_stream_size, max_pdf_decoded_size.
#[pyclass(name = "Limits", module = "markkit", frozen, get_all)]
#[derive(Clone)]
struct Limits {
    max_input_size: usize,
    max_output_size: usize,
    timeout: f64,
    max_nesting_depth: usize,
    max_nodes: usize,
    max_archive_entries: usize,
    max_archive_entry_size: u64,
    max_archive_total_size: u64,
    max_sheet_rows: usize,
    max_sheet_cols: usize,
    max_spreadsheet_cells: u64,
    max_redirects: usize,
    max_pdf_pages: usize,
    max_pdf_stream_size: usize,
    max_pdf_decoded_size: usize,
}

#[pymethods]
impl Limits {
    #[new]
    #[pyo3(signature = (
        *,
        max_input_size=None,
        max_output_size=None,
        timeout=None,
        max_nesting_depth=None,
        max_nodes=None,
        max_archive_entries=None,
        max_archive_entry_size=None,
        max_archive_total_size=None,
        max_sheet_rows=None,
        max_sheet_cols=None,
        max_spreadsheet_cells=None,
        max_redirects=None,
        max_pdf_pages=None,
        max_pdf_stream_size=None,
        max_pdf_decoded_size=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        max_input_size: Option<usize>,
        max_output_size: Option<usize>,
        timeout: Option<f64>,
        max_nesting_depth: Option<usize>,
        max_nodes: Option<usize>,
        max_archive_entries: Option<usize>,
        max_archive_entry_size: Option<u64>,
        max_archive_total_size: Option<u64>,
        max_sheet_rows: Option<usize>,
        max_sheet_cols: Option<usize>,
        max_spreadsheet_cells: Option<u64>,
        max_redirects: Option<usize>,
        max_pdf_pages: Option<usize>,
        max_pdf_stream_size: Option<usize>,
        max_pdf_decoded_size: Option<usize>,
    ) -> PyResult<Self> {
        let d = markkit::Limits::default();
        if let Some(t) = timeout {
            Duration::try_from_secs_f64(t)
                .map_err(|e| PyValueError::new_err(format!("timeout: {e}")))?;
        }
        Ok(Self {
            max_input_size: max_input_size.unwrap_or(d.max_input_size),
            max_output_size: max_output_size.unwrap_or(d.max_output_size),
            timeout: timeout.unwrap_or(d.timeout.as_secs_f64()),
            max_nesting_depth: max_nesting_depth.unwrap_or(d.max_nesting_depth),
            max_nodes: max_nodes.unwrap_or(d.max_nodes),
            max_archive_entries: max_archive_entries.unwrap_or(d.max_archive_entries),
            max_archive_entry_size: max_archive_entry_size.unwrap_or(d.max_archive_entry_size),
            max_archive_total_size: max_archive_total_size.unwrap_or(d.max_archive_total_size),
            max_sheet_rows: max_sheet_rows.unwrap_or(d.max_sheet_rows),
            max_sheet_cols: max_sheet_cols.unwrap_or(d.max_sheet_cols),
            max_spreadsheet_cells: max_spreadsheet_cells.unwrap_or(d.max_spreadsheet_cells),
            max_redirects: max_redirects.unwrap_or(d.max_redirects),
            max_pdf_pages: max_pdf_pages.unwrap_or(d.max_pdf_pages),
            max_pdf_stream_size: max_pdf_stream_size.unwrap_or(d.max_pdf_stream_size),
            max_pdf_decoded_size: max_pdf_decoded_size.unwrap_or(d.max_pdf_decoded_size),
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "Limits(max_input_size={}, max_output_size={}, timeout={}, max_nesting_depth={}, \
             max_nodes={}, max_archive_entries={}, max_archive_entry_size={}, \
             max_archive_total_size={}, max_sheet_rows={}, max_sheet_cols={}, \
             max_spreadsheet_cells={}, max_redirects={}, max_pdf_pages={}, \
             max_pdf_stream_size={}, max_pdf_decoded_size={})",
            self.max_input_size,
            self.max_output_size,
            self.timeout,
            self.max_nesting_depth,
            self.max_nodes,
            self.max_archive_entries,
            self.max_archive_entry_size,
            self.max_archive_total_size,
            self.max_sheet_rows,
            self.max_sheet_cols,
            self.max_spreadsheet_cells,
            self.max_redirects,
            self.max_pdf_pages,
            self.max_pdf_stream_size,
            self.max_pdf_decoded_size
        )
    }
}

impl Limits {
    fn to_core(&self) -> markkit::Limits {
        markkit::Limits::default()
            .max_input_size(self.max_input_size)
            .max_output_size(self.max_output_size)
            // Validated in `new`.
            .timeout(Duration::try_from_secs_f64(self.timeout).unwrap_or(Duration::ZERO))
            .max_nesting_depth(self.max_nesting_depth)
            .max_nodes(self.max_nodes)
            .max_archive_entries(self.max_archive_entries)
            .max_archive_entry_size(self.max_archive_entry_size)
            .max_archive_total_size(self.max_archive_total_size)
            .max_sheet_rows(self.max_sheet_rows)
            .max_sheet_cols(self.max_sheet_cols)
            .max_spreadsheet_cells(self.max_spreadsheet_cells)
            .max_redirects(self.max_redirects)
            .max_pdf_pages(self.max_pdf_pages)
            .max_pdf_stream_size(self.max_pdf_stream_size)
            .max_pdf_decoded_size(self.max_pdf_decoded_size)
    }
}

fn builder(limits: Option<&Limits>) -> markkit::MarkkitBuilder {
    let b = markkit::Markkit::builder();
    match limits {
        Some(l) => b.limits(l.to_core()),
        None => b,
    }
}

fn err(e: markkit::Error) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

/// Extract markdown from an HTML string (async).
#[pyfunction]
#[pyo3(signature = (html, *, limits=None))]
fn extract_html<'py>(
    py: Python<'py>,
    html: String,
    limits: Option<PyRef<'py, Limits>>,
) -> PyResult<Bound<'py, PyAny>> {
    let m = builder(limits.as_deref()).build();
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        m.extract_html(&html).await.map_err(err)
    })
}

/// Extract markdown from an HTML string (synchronous).
#[pyfunction]
#[pyo3(signature = (html, *, limits=None))]
fn extract_html_sync(html: String, limits: Option<PyRef<'_, Limits>>) -> PyResult<String> {
    builder(limits.as_deref())
        .build()
        .extract_html_sync(&html)
        .map_err(err)
}

/// Extract markdown from an HTML string with configuration options (synchronous).
///
/// Keyword arguments:
///   include_selectors: list of CSS selectors to include
///   exclude_selectors: list of CSS selectors to exclude
///   max_input_size: maximum input size in bytes (overrides limits.max_input_size)
///   base_url: absolute URL relative links and images resolve against
///   main_content: keep only the detected article body (default False)
///   limits: markkit.Limits
#[pyfunction]
#[pyo3(signature = (html, *, include_selectors=None, exclude_selectors=None, max_input_size=None, base_url=None, main_content=false, limits=None))]
fn extract_html_sync_with_options(
    html: String,
    include_selectors: Option<Vec<String>>,
    exclude_selectors: Option<Vec<String>>,
    max_input_size: Option<usize>,
    base_url: Option<String>,
    main_content: bool,
    limits: Option<PyRef<'_, Limits>>,
) -> PyResult<String> {
    let mut builder = builder(limits.as_deref()).main_content(main_content);
    if let Some(base) = base_url {
        builder = builder.base_url(base);
    }
    if let Some(size) = max_input_size {
        builder = builder.max_input_size(size);
    }
    if let Some(selectors) = include_selectors {
        for sel in &selectors {
            builder = builder.include_selector(sel);
        }
    }
    if let Some(selectors) = exclude_selectors {
        for sel in &selectors {
            builder = builder.exclude_selector(sel);
        }
    }
    builder.build().extract_html_sync(&html).map_err(err)
}

/// Extract markdown from a URL.
#[pyfunction]
#[pyo3(signature = (url, *, limits=None))]
fn extract_url<'py>(
    py: Python<'py>,
    url: String,
    limits: Option<PyRef<'py, Limits>>,
) -> PyResult<Bound<'py, PyAny>> {
    let m = builder(limits.as_deref()).build();
    pyo3_async_runtimes::tokio::future_into_py(
        py,
        async move { m.extract_url(&url).await.map_err(err) },
    )
}

/// Extract markdown from bytes with optional content type.
#[pyfunction]
#[pyo3(signature = (data, content_type=None, *, limits=None))]
fn extract_bytes<'py>(
    py: Python<'py>,
    data: Vec<u8>,
    content_type: Option<String>,
    limits: Option<PyRef<'py, Limits>>,
) -> PyResult<Bound<'py, PyAny>> {
    let m = builder(limits.as_deref()).build();
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        m.extract_bytes(&data, content_type.as_deref())
            .await
            .map_err(err)
    })
}

/// Extract document bytes to a Python dictionary containing Markdown, metadata
/// and optional PDF layout. OCR is local and opt-in; helpers must be installed.
#[pyfunction]
#[pyo3(signature = (data, *, file_name=None, content_type=None, pdf_layout=false, pdf_remove_repeated_headers=false, pdf_ocr=false, pdf_ocr_language="eng", pdf_ocr_backend="tesseract", pdf_ocr_detection_model=None, pdf_ocr_recognition_model=None, limits=None))]
#[allow(clippy::too_many_arguments)]
fn extract_document_sync<'py>(
    py: Python<'py>,
    data: Vec<u8>,
    file_name: Option<String>,
    content_type: Option<String>,
    pdf_layout: bool,
    pdf_remove_repeated_headers: bool,
    pdf_ocr: bool,
    pdf_ocr_language: &str,
    pdf_ocr_backend: &str,
    pdf_ocr_detection_model: Option<String>,
    pdf_ocr_recognition_model: Option<String>,
    limits: Option<PyRef<'py, Limits>>,
) -> PyResult<Bound<'py, PyAny>> {
    let mut config = builder(limits.as_deref())
        .pdf_layout(pdf_layout)
        .pdf_remove_repeated_headers(pdf_remove_repeated_headers)
        .pdf_ocr(pdf_ocr)
        .pdf_ocr_language(pdf_ocr_language)
        .pdf_ocr_backend(pdf_ocr_backend.parse().map_err(PyValueError::new_err)?);
    if let Some(path) = pdf_ocr_detection_model {
        config = config.pdf_ocr_detection_model(path);
    }
    if let Some(path) = pdf_ocr_recognition_model {
        config = config.pdf_ocr_recognition_model(path);
    }
    let e = config
        .build()
        .extract_document_sync(
            &data,
            markkit::DocumentHint {
                file_name: file_name.as_deref(),
                content_type: content_type.as_deref(),
            },
        )
        .map_err(err)?;
    let json = serde_json::to_string(&e).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    py.import("json")?.call_method1("loads", (json,))
}

#[pymodule]
#[pyo3(name = "markkit")]
fn markkit_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Limits>()?;
    m.add_function(wrap_pyfunction!(extract_document_sync, m)?)?;
    m.add_function(wrap_pyfunction!(extract_html, m)?)?;
    m.add_function(wrap_pyfunction!(extract_html_sync, m)?)?;
    m.add_function(wrap_pyfunction!(extract_html_sync_with_options, m)?)?;
    m.add_function(wrap_pyfunction!(extract_url, m)?)?;
    m.add_function(wrap_pyfunction!(extract_bytes, m)?)?;
    Ok(())
}
