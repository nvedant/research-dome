//! JSON shapes returned by the API. Field names and types intentionally mirror
//! the original FastAPI/Pydantic schemas so the UI works without changes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub paper_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaperOut {
    pub id: i64,
    pub project_id: i64,
    pub category_id: Option<i64>,
    pub title: String,
    pub original_filename: String,
    pub citation_number: i64,
    pub file_size: i64,
    pub uploaded_at: String,
    pub category_name: String,
    pub annotation_count: i64,
}

/// A paper row including its private storage path (never serialised to the UI).
#[derive(Debug, Clone)]
pub struct PaperRecord {
    pub out: PaperOut,
    pub storage_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Annotation {
    pub id: i64,
    pub paper_id: i64,
    pub page_number: i64,
    pub annotation_type: String,
    pub text: String,
    pub color: String,
    pub rects: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentenceOut {
    pub id: i64,
    pub paper_id: i64,
    pub citation_number: i64,
    pub paper_title: String,
    pub category_name: String,
    pub page_number: i64,
    pub text: String,
    pub color: String,
    pub created_at: String,
    pub rects: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageOut {
    pub id: i64,
    pub project_id: i64,
    pub paper_id: Option<i64>,
    pub title: String,
    pub page_number: Option<i64>,
    pub created_at: String,
    pub paper_title: Option<String>,
    pub category_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImageRecord {
    pub out: ImageOut,
    pub storage_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LatexOut {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub compiled: bool,
    pub last_compile_log: String,
}

#[derive(Debug, Clone)]
pub struct LatexRecord {
    pub out: LatexOut,
    pub storage_dir: String,
}

// ---------------------------------------------------------------- request bodies

#[derive(Debug, Deserialize)]
pub struct ProjectIn {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct CategoryIn {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

fn default_color() -> String {
    "#ffe66d".to_string()
}

#[derive(Debug, Deserialize)]
pub struct AnnotationIn {
    pub page_number: i64,
    pub annotation_type: String,
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub rects: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct LatexCreate {
    pub title: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Deserialize)]
pub struct LatexSave {
    pub source: String,
}

#[derive(Debug, Deserialize)]
pub struct LatexFileSave {
    pub content: String,
}
