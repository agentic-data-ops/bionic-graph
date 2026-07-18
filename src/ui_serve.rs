use axum::{
    body::Body,
    extract::Path,
    http::{header, StatusCode, Response},
    response::IntoResponse,
};
use rust_embed::Embed;

/// Embed the entire compiled frontend directory into the binary.
#[derive(Embed)]
#[folder = "src/ui/dist"]
struct UiAssets;

/// Serve index.html for bare `/ui` or `/ui/` paths.
pub async fn ui_root_handler() -> impl IntoResponse {
    serve_embedded_file("")
}

/// Handle `/ui/*path` — serve embedded files with correct MIME types.
/// Falls back to `index.html` for SPA client-side routing.
pub async fn ui_handler(Path(path): Path<String>) -> impl IntoResponse {
    // Strip leading slash; empty means /ui/
    serve_embedded_file(path.trim_start_matches('/'))
}

fn serve_embedded_file(clean_path: &str) -> impl IntoResponse {

    // Try the exact path
    if let Some(file) = UiAssets::get(clean_path) {
        let mime = mime_guess::from_path(clean_path).first_or_octet_stream();
        return Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(file.data.to_vec()))
            .unwrap();
    }

    // SPA fallback: if the path looks like a navigation route (no file extension),
    // serve index.html so React Router can handle it.
    let has_extension = clean_path.contains('.');
    if !has_extension {
        if let Some(file) = UiAssets::get("index.html") {
            return Response::builder()
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Body::from(file.data.to_vec()))
                .unwrap();
        }
    }

    // 404
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Not found"))
        .unwrap()
}
