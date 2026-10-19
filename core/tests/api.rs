//! End-to-end API tests: the real router, a real SQLite file and real storage,
//! driven in-process (no network).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use lopdf::{dictionary, Document, Object, Stream};
use research_dome_core::api::{build_router, AppState};
use research_dome_core::db::Db;
use research_dome_core::storage::Storage;
use serde_json::{json, Value};
use tower::ServiceExt;

struct Harness {
    app: Router,
    _dir: tempfile::TempDir,
    storage: Storage,
}

fn harness(token: Option<&str>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path().join("storage")).unwrap();
    let db = Db::open(&dir.path().join("test.sqlite")).unwrap();
    let state = AppState::new(db, storage.clone(), token.map(str::to_owned));
    Harness { app: build_router(state), _dir: dir, storage }
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, bytes, headers)
}

async fn json_req(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let (status, bytes, _) = send(app, builder.body(body).unwrap()).await;
    let value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, value)
}

/// Build a multipart/form-data body by hand.
fn multipart(fields: &[(&str, &str)], file: Option<(&str, &str, &str, &[u8])>) -> (String, Vec<u8>) {
    let boundary = "----domeboundary7MA4YWxkTrZu0gW";
    let mut body: Vec<u8> = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    if let Some((name, filename, content_type, data)) = file {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn post_multipart(app: &Router, uri: &str, fields: &[(&str, &str)], file: Option<(&str, &str, &str, &[u8])>) -> (StatusCode, Value) {
    let (ct, body) = multipart(fields, file);
    let req = Request::builder().method("POST").uri(uri).header("content-type", ct).body(Body::from(body)).unwrap();
    let (status, bytes, _) = send(app, req).await;
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn make_pdf(pages: usize, rotate: Option<i64>) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
    let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
    let mut kids = Vec::new();
    for n in 0..pages {
        let content = format!("BT /F1 24 Tf 72 700 Td (Research Dome test page {}) Tj ET", n + 1);
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let mut page = dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        };
        if let Some(r) = rotate {
            page.set("Rotate", r);
        }
        kids.push(Object::Reference(doc.add_object(page)));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => pages as i64, "Resources" => resources_id,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

async fn seed(h: &Harness) -> (i64, i64) {
    let (s, p) = json_req(&h.app, "POST", "/api/projects", Some(json!({"name": "  Quantum  ", "description": " QC "}))).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(p["name"], "Quantum");
    assert_eq!(p["description"], "QC");
    let pid = p["id"].as_i64().unwrap();
    let (s, c) = json_req(&h.app, "POST", &format!("/api/projects/{pid}/categories"), Some(json!({"name": "Methods"}))).await;
    assert_eq!(s, StatusCode::CREATED);
    (pid, c["id"].as_i64().unwrap())
}

#[tokio::test]
async fn health_and_token_gate() {
    let h = harness(Some("secret"));
    let (s, _) = json_req(&h.app, "GET", "/api/health", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let req = Request::builder().uri("/api/health").header("x-dome-token", "secret").body(Body::empty()).unwrap();
    let (s, bytes, _) = send(&h.app, req).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap()["status"], "ok");
    // query-string token (used by <img> / PDF URLs)
    let req = Request::builder().uri("/api/health?token=secret").body(Body::empty()).unwrap();
    assert_eq!(send(&h.app, req).await.0, StatusCode::OK);
}

#[tokio::test]
async fn projects_and_categories() {
    let h = harness(None);
    let (pid, cid) = seed(&h).await;

    let (s, v) = json_req(&h.app, "POST", "/api/projects", Some(json!({"name": "   "}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(v["detail"].as_str().unwrap().contains("required"));

    let (s, _) = json_req(&h.app, "POST", &format!("/api/projects/{pid}/categories"), Some(json!({"name": "METHODS"}))).await;
    assert_eq!(s, StatusCode::CONFLICT);

    let (s, v) = json_req(&h.app, "GET", &format!("/api/projects/{pid}/categories"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v[0]["id"], cid);
    assert_eq!(v[0]["paper_count"], 0);

    assert_eq!(json_req(&h.app, "GET", "/api/projects/999", None).await.0, StatusCode::NOT_FOUND);
    let (s, v) = json_req(&h.app, "GET", "/api/projects", None).await;
    assert_eq!((s, v.as_array().unwrap().len()), (StatusCode::OK, 1));
}

#[tokio::test]
async fn paper_lifecycle_annotations_and_export() {
    let h = harness(None);
    let (pid, cid) = seed(&h).await;
    let pdf = make_pdf(3, None);

    // validation
    let (s, _) = post_multipart(&h.app, "/api/papers", &[("project_id", &pid.to_string()), ("category_id", &cid.to_string())], Some(("file", "notes.txt", "text/plain", b"hello"))).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let (s, _) = post_multipart(&h.app, "/api/papers", &[("project_id", &pid.to_string()), ("category_id", &cid.to_string())], Some(("file", "fake.pdf", "application/pdf", b"not a pdf"))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post_multipart(&h.app, "/api/papers", &[("project_id", &pid.to_string()), ("category_id", "9999")], Some(("file", "a.pdf", "application/pdf", &pdf))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // two uploads -> citation numbers 1 and 2
    let (s, p1) = post_multipart(&h.app, "/api/papers", &[("project_id", &pid.to_string()), ("category_id", &cid.to_string())], Some(("file", "My First Paper.pdf", "application/pdf", &pdf))).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(p1["citation_number"], 1);
    assert_eq!(p1["title"], "My_First_Paper");
    assert_eq!(p1["category_name"], "Methods");
    let (_, p2) = post_multipart(&h.app, "/api/papers", &[("project_id", &pid.to_string()), ("category_id", &cid.to_string()), ("title", "Second")], Some(("file", "b.pdf", "application/pdf", &pdf))).await;
    assert_eq!(p2["citation_number"], 2);
    assert_eq!(p2["title"], "Second");
    let paper = p1["id"].as_i64().unwrap();

    // listing, search, category filter
    let (_, list) = json_req(&h.app, "GET", &format!("/api/projects/{pid}/papers"), None).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (_, list) = json_req(&h.app, "GET", &format!("/api/projects/{pid}/papers?q=second"), None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let (_, list) = json_req(&h.app, "GET", &format!("/api/projects/{pid}/papers?category_id=12345"), None).await;
    assert_eq!(list.as_array().unwrap().len(), 0);

    // file serving
    let (s, bytes, headers) = send(&h.app, Request::builder().uri(format!("/api/papers/{paper}/file")).body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(bytes, pdf);
    assert!(headers["content-disposition"].to_str().unwrap().starts_with("inline"));

    // patch title + move to Uncategorized + invalid category
    let (s, v) = json_req(&h.app, "PATCH", &format!("/api/papers/{paper}"), Some(json!({"title": " Renamed ", "category_id": null}))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["title"], "Renamed");
    assert_eq!(v["category_name"], "Uncategorized");
    assert!(v["category_id"].is_null());
    let (s, _) = json_req(&h.app, "PATCH", &format!("/api/papers/{paper}"), Some(json!({"category_id": 424242}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = json_req(&h.app, "PATCH", &format!("/api/papers/{paper}"), Some(json!({"category_id": cid}))).await;
    assert_eq!(s, StatusCode::OK);

    // annotations: validation
    let url = format!("/api/papers/{paper}/annotations");
    let rect = json!({"x": 0.1, "y": 0.1, "width": 0.5, "height": 0.05});
    let (s, _) = json_req(&h.app, "POST", &url, Some(json!({"page_number": 1, "annotation_type": "sentence", "text": "", "rects": [rect]}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = json_req(&h.app, "POST", &url, Some(json!({"page_number": 1, "annotation_type": "rectangle", "rects": []}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = json_req(&h.app, "POST", &url, Some(json!({"page_number": 1, "annotation_type": "rectangle", "color": "red", "rects": [rect]}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = json_req(&h.app, "POST", &url, Some(json!({"page_number": 1, "annotation_type": "rectangle", "rects": [{"x": 2, "y": 0, "width": 0.1, "height": 0.1}]}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = json_req(&h.app, "POST", &url, Some(json!({"page_number": 0, "annotation_type": "rectangle", "rects": [rect]}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // annotations: create
    let (s, a1) = json_req(&h.app, "POST", &url, Some(json!({"page_number": 1, "annotation_type": "sentence", "text": " Important claim. ", "color": "#b7f0d0", "rects": [rect]}))).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(a1["text"], "Important claim.");
    assert_eq!(a1["rects"][0]["width"], 0.5);
    let (s, _) = json_req(&h.app, "POST", &url, Some(json!({"page_number": 2, "annotation_type": "rectangle", "text": "Fig", "rects": [rect]}))).await;
    assert_eq!(s, StatusCode::CREATED);

    let (_, anns) = json_req(&h.app, "GET", &url, None).await;
    assert_eq!(anns.as_array().unwrap().len(), 2);
    let (_, sentences) = json_req(&h.app, "GET", &format!("/api/projects/{pid}/important-sentences"), None).await;
    assert_eq!(sentences.as_array().unwrap().len(), 1);
    assert_eq!(sentences[0]["citation_number"], 1);
    assert_eq!(sentences[0]["category_name"], "Methods");
    let (_, listed) = json_req(&h.app, "GET", &format!("/api/projects/{pid}/papers"), None).await;
    assert_eq!(listed[0]["annotation_count"], 2);

    // annotated PDF: parses, same page count, overlay streams present
    let (s, out, headers) = send(&h.app, Request::builder().uri(format!("/api/papers/{paper}/annotated-file")).body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    assert!(headers["content-disposition"].to_str().unwrap().contains("_annotated.pdf"));
    let annotated = Document::load_mem(&out).expect("annotated PDF must parse");
    assert_eq!(annotated.get_pages().len(), 3);
    let page1 = *annotated.get_pages().get(&1).unwrap();
    let content = annotated.get_page_content(page1);
    let text = String::from_utf8_lossy(&content);
    assert!(text.contains("/RDFill gs"), "overlay missing: {text}");
    assert!(text.contains("Research Dome test page 1"), "original content lost");
    let page3 = *annotated.get_pages().get(&3).unwrap();
    assert!(!String::from_utf8_lossy(&annotated.get_page_content(page3)).contains("RDFill"));
    std::fs::write(std::env::temp_dir().join("dome_annotated_sample.pdf"), &out).unwrap();

    // deleting an annotation, then the paper (file must go too)
    let aid = a1["id"].as_i64().unwrap();
    assert_eq!(json_req(&h.app, "DELETE", &format!("/api/annotations/{aid}"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(json_req(&h.app, "DELETE", &format!("/api/annotations/{aid}"), None).await.0, StatusCode::NOT_FOUND);
    let stored = h.storage.papers_dir(pid);
    assert_eq!(std::fs::read_dir(&stored).unwrap().count(), 2);
    assert_eq!(json_req(&h.app, "DELETE", &format!("/api/papers/{paper}"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(std::fs::read_dir(&stored).unwrap().count(), 1);
    assert_eq!(json_req(&h.app, "GET", &format!("/api/papers/{paper}"), None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rotated_pages_are_exported() {
    let h = harness(None);
    let (pid, cid) = seed(&h).await;
    let (_, p) = post_multipart(&h.app, "/api/papers", &[("project_id", &pid.to_string()), ("category_id", &cid.to_string())], Some(("file", "r.pdf", "application/pdf", &make_pdf(1, Some(90))))).await;
    let paper = p["id"].as_i64().unwrap();
    let rect = json!({"x": 0.0, "y": 0.0, "width": 0.5, "height": 0.5});
    json_req(&h.app, "POST", &format!("/api/papers/{paper}/annotations"), Some(json!({"page_number": 1, "annotation_type": "rectangle", "rects": [rect]}))).await;
    let (s, out, _) = send(&h.app, Request::builder().uri(format!("/api/papers/{paper}/annotated-file")).body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    std::fs::write(std::env::temp_dir().join("dome_annotated_rotated.pdf"), &out).unwrap();
    assert!(Document::load_mem(&out).is_ok());
}

#[tokio::test]
async fn category_deletion_modes() {
    let h = harness(None);
    let (pid, cid) = seed(&h).await;
    let pdf = make_pdf(1, None);
    let (_, paper) = post_multipart(&h.app, "/api/papers", &[("project_id", &pid.to_string()), ("category_id", &cid.to_string())], Some(("file", "a.pdf", "application/pdf", &pdf))).await;
    let (s, _) = json_req(&h.app, "DELETE", &format!("/api/categories/{cid}?delete_papers=false"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, p) = json_req(&h.app, "GET", &format!("/api/papers/{}", paper["id"]), None).await;
    assert_eq!(p["category_name"], "Uncategorized");

    let (_, c2) = json_req(&h.app, "POST", &format!("/api/projects/{pid}/categories"), Some(json!({"name": "Eval"}))).await;
    let c2 = c2["id"].as_i64().unwrap();
    let (_, paper2) = post_multipart(&h.app, "/api/papers", &[("project_id", &pid.to_string()), ("category_id", &c2.to_string())], Some(("file", "b.pdf", "application/pdf", &pdf))).await;
    assert_eq!(json_req(&h.app, "DELETE", &format!("/api/categories/{c2}?delete_papers=true"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(json_req(&h.app, "GET", &format!("/api/papers/{}", paper2["id"]), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(json_req(&h.app, "GET", &format!("/api/papers/{}", paper["id"]), None).await.0, StatusCode::OK);
}

fn tiny_png() -> Vec<u8> {
    // 1x1 transparent PNG
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41,
        0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

#[tokio::test]
async fn images_and_project_deletion_cleans_files() {
    let h = harness(None);
    let (pid, cid) = seed(&h).await;
    let (_, paper) = post_multipart(&h.app, "/api/papers", &[("project_id", &pid.to_string()), ("category_id", &cid.to_string())], Some(("file", "a.pdf", "application/pdf", &make_pdf(1, None)))).await;
    let png = tiny_png();

    let (s, _) = post_multipart(&h.app, "/api/images", &[("project_id", &pid.to_string())], Some(("file", "x.gif", "image/gif", b"GIF89a"))).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let (s, img) = post_multipart(
        &h.app, "/api/images",
        &[("project_id", &pid.to_string()), ("paper_id", &paper["id"].to_string()), ("page_number", "2"), ("title", "Circuit diagram")],
        Some(("file", "crop.png", "image/png", &png)),
    ).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(img["title"], "Circuit diagram");
    assert_eq!(img["category_name"], "Methods");
    assert_eq!(img["page_number"], 2);
    let (_, standalone) = post_multipart(&h.app, "/api/images", &[("project_id", &pid.to_string())], Some(("file", "c.png", "image/png", &png))).await;
    assert_eq!(standalone["title"], "Research clipping");
    assert!(standalone["category_name"].is_null());

    let (_, list) = json_req(&h.app, "GET", &format!("/api/projects/{pid}/images"), None).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (s, bytes, headers) = send(&h.app, Request::builder().uri(format!("/api/images/{}/file", img["id"])).body(Body::empty()).unwrap()).await;
    assert_eq!((s, bytes), (StatusCode::OK, png.clone()));
    assert_eq!(headers["content-type"], "image/png");

    // Deleting the paper keeps the figure (paper link is cleared).
    json_req(&h.app, "DELETE", &format!("/api/papers/{}", paper["id"]), None).await;
    let (_, list) = json_req(&h.app, "GET", &format!("/api/projects/{pid}/images"), None).await;
    assert_eq!(list.as_array().unwrap().len(), 2);

    // LaTeX workspace so project deletion has something to remove.
    json_req(&h.app, "POST", &format!("/api/projects/{pid}/latex-documents"), Some(json!({"title": "Draft"}))).await;
    assert!(h.storage.latex_dir(pid).exists());
    assert_eq!(json_req(&h.app, "DELETE", &format!("/api/projects/{pid}"), None).await.0, StatusCode::NO_CONTENT);
    assert!(!h.storage.images_dir(pid).exists());
    assert!(!h.storage.papers_dir(pid).exists());
    assert!(!h.storage.latex_dir(pid).exists());
    assert_eq!(json_req(&h.app, "GET", &format!("/api/projects/{pid}"), None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn latex_workspace_files_and_build() {
    let h = harness(None);
    let (pid, _) = seed(&h).await;
    let (s, doc) = json_req(&h.app, "POST", &format!("/api/projects/{pid}/latex-documents"), Some(json!({"title": "Main paper", "source": ""}))).await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(doc["source"].as_str().unwrap().contains("\\documentclass"));
    let id = doc["id"].as_i64().unwrap();
    let base = format!("/api/latex-documents/{id}");

    let (_, files) = json_req(&h.app, "GET", &format!("{base}/files"), None).await;
    assert_eq!(files["files"][0]["path"], "main.tex");

    // traversal and type rules
    assert_eq!(json_req(&h.app, "GET", &format!("{base}/file?path=..%2F..%2Fsecret.tex"), None).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(json_req(&h.app, "PUT", &format!("{base}/file?path=evil.exe"), Some(json!({"content": "x"}))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(json_req(&h.app, "PUT", &format!("{base}/file?path=main.pdf"), Some(json!({"content": "x"}))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(json_req(&h.app, "DELETE", &format!("{base}/file?path=main.tex"), None).await.0, StatusCode::BAD_REQUEST);

    // create a bib + nested tex, read back, upload an image asset
    let (s, v) = json_req(&h.app, "PUT", &format!("{base}/file?path=references.bib"), Some(json!({"content": "@article{a,title={T},author={A},year={2020},journal={J}}"}))).await;
    assert_eq!((s, v["saved"].clone()), (StatusCode::OK, json!(true)));
    json_req(&h.app, "PUT", &format!("{base}/file?path=sections%2Fintro.tex"), Some(json!({"content": "Hello from intro."}))).await;
    let (_, got) = json_req(&h.app, "GET", &format!("{base}/file?path=sections%2Fintro.tex"), None).await;
    assert_eq!(got["content"], "Hello from intro.");
    assert_eq!(got["path"], "sections/intro.tex");
    let (s, up) = post_multipart(&h.app, &format!("{base}/files"), &[("path", "figures/dot.png")], Some(("file", "dot.png", "image/png", &tiny_png()))).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(up["path"], "figures/dot.png");
    let (s, bytes, headers) = send(&h.app, Request::builder().uri(format!("{base}/asset?path=figures%2Fdot.png")).body(Body::empty()).unwrap()).await;
    assert_eq!((s, bytes.len()), (StatusCode::OK, tiny_png().len()));
    assert_eq!(headers["content-type"], "image/png");
    assert_eq!(json_req(&h.app, "GET", &format!("{base}/asset?path=main.tex"), None).await.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let (_, files) = json_req(&h.app, "GET", &format!("{base}/files"), None).await;
    let paths: Vec<&str> = files["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec!["main.tex", "figures/dot.png", "references.bib", "sections/intro.tex"]);

    // save main.tex through the document endpoint and download the source
    json_req(&h.app, "PUT", &base, Some(json!({"source": "\\documentclass{article}\\begin{document}Hi \\cite{a}\\input{sections/intro}\\bibliographystyle{plain}\\bibliography{references}\\end{document}"}))).await;
    let (s, src, _) = send(&h.app, Request::builder().uri(format!("{base}/source")).body(Body::empty()).unwrap()).await;
    assert_eq!(s, StatusCode::OK);
    assert!(String::from_utf8(src).unwrap().contains("\\input{sections/intro}"));

    // no PDF before a build
    assert_eq!(json_req(&h.app, "GET", &format!("{base}/pdf"), None).await.0, StatusCode::NOT_FOUND);

    // Build only when a TeX distribution is installed (skipped otherwise).
    if research_dome_core::latex::find_executable("pdflatex").is_some() {
        let (s, built) = json_req(&h.app, "POST", &format!("{base}/compile"), None).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(built["success"], true, "build log: {}", built["log"]);
        assert_eq!(built["compiled"], true);
        let (s, pdf, _) = send(&h.app, Request::builder().uri(format!("{base}/pdf")).body(Body::empty()).unwrap()).await;
        assert_eq!(s, StatusCode::OK);
        assert!(pdf.starts_with(b"%PDF-"));
        let (_, d) = json_req(&h.app, "GET", &base, None).await;
        assert_eq!(d["compiled"], true);

        // A broken document reports a failure and keeps the previous PDF.
        json_req(&h.app, "PUT", &base, Some(json!({"source": "\\documentclass{article}\\begin{document}\\undefinedcommand\\end{document}"}))).await;
        let (_, bad) = json_req(&h.app, "POST", &format!("{base}/compile"), None).await;
        assert_eq!(bad["success"], false);
        assert!(bad["log"].as_str().unwrap().contains("Undefined control sequence"));
        assert_eq!(json_req(&h.app, "GET", &format!("{base}/pdf"), None).await.0, StatusCode::OK);
    } else {
        eprintln!("pdflatex not installed: skipping the build part of the test");
    }

    // file deletion + workspace deletion
    assert_eq!(json_req(&h.app, "DELETE", &format!("{base}/file?path=references.bib"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(json_req(&h.app, "DELETE", &format!("{base}/file?path=references.bib"), None).await.0, StatusCode::NOT_FOUND);
    let (_, list) = json_req(&h.app, "GET", &format!("/api/projects/{pid}/latex-documents"), None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(json_req(&h.app, "DELETE", &base, None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(json_req(&h.app, "GET", &base, None).await.0, StatusCode::NOT_FOUND);
}
