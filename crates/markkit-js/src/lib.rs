use napi_derive::napi;

/// Extract markdown from an HTML string.
#[napi]
pub async fn extract_html(html: String) -> napi::Result<String> {
    let m = markkit::Markkit::builder().build();
    m.extract_html(&html)
        .await
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}

/// Extract markdown from a URL.
#[napi]
pub async fn extract_url(url: String) -> napi::Result<String> {
    let m = markkit::Markkit::builder().build();
    m.extract_url(&url)
        .await
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}

/// Extract markdown from a buffer with optional content type.
#[napi]
pub async fn extract_bytes(
    data: napi::bindgen_prelude::Buffer,
    content_type: Option<String>,
) -> napi::Result<String> {
    let m = markkit::Markkit::builder().build();
    m.extract_bytes(&data, content_type.as_deref())
        .await
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}
