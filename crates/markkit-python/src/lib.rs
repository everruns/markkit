use pyo3::prelude::*;

/// Extract markdown from an HTML string (async).
#[pyfunction]
fn extract_html(py: Python<'_>, html: String) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let m = markkit::Markkit::builder().build();
        m.extract_html(&html)
            .await
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    })
}

/// Extract markdown from an HTML string (synchronous).
#[pyfunction]
fn extract_html_sync(html: String) -> PyResult<String> {
    let m = markkit::Markkit::builder().build();
    m.extract_html_sync(&html)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
}

/// Extract markdown from an HTML string with configuration options (synchronous).
///
/// Keyword arguments:
///   include_selectors: list of CSS selectors to include
///   exclude_selectors: list of CSS selectors to exclude
///   max_input_size: maximum input size in bytes
#[pyfunction]
#[pyo3(signature = (html, *, include_selectors=None, exclude_selectors=None, max_input_size=None))]
fn extract_html_sync_with_options(
    html: String,
    include_selectors: Option<Vec<String>>,
    exclude_selectors: Option<Vec<String>>,
    max_input_size: Option<usize>,
) -> PyResult<String> {
    let mut builder = markkit::Markkit::builder();
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
    let m = builder.build();
    m.extract_html_sync(&html)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
}

/// Extract markdown from a URL.
#[pyfunction]
fn extract_url(py: Python<'_>, url: String) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let m = markkit::Markkit::builder().build();
        m.extract_url(&url)
            .await
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    })
}

/// Extract markdown from bytes with optional content type.
#[pyfunction]
fn extract_bytes(
    py: Python<'_>,
    data: Vec<u8>,
    content_type: Option<String>,
) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let m = markkit::Markkit::builder().build();
        m.extract_bytes(&data, content_type.as_deref())
            .await
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    })
}

#[pymodule]
fn markkit_python(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(extract_html, m)?)?;
    m.add_function(wrap_pyfunction!(extract_html_sync, m)?)?;
    m.add_function(wrap_pyfunction!(extract_html_sync_with_options, m)?)?;
    m.add_function(wrap_pyfunction!(extract_url, m)?)?;
    m.add_function(wrap_pyfunction!(extract_bytes, m)?)?;
    Ok(())
}
