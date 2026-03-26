use pyo3::prelude::*;

/// Extract markdown from an HTML string.
#[pyfunction]
fn extract_html(py: Python<'_>, html: String) -> PyResult<Bound<'_, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let m = markkit::Markkit::builder().build();
        m.extract_html(&html)
            .await
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    })
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
fn extract_bytes(py: Python<'_>, data: Vec<u8>, content_type: Option<String>) -> PyResult<Bound<'_, PyAny>> {
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
    m.add_function(wrap_pyfunction!(extract_url, m)?)?;
    m.add_function(wrap_pyfunction!(extract_bytes, m)?)?;
    Ok(())
}
