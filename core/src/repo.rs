//! Database access. Plain synchronous functions over a `rusqlite::Connection`;
//! the API layer runs them through `Db::call` on the blocking thread pool.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::util::now_iso;

// ------------------------------------------------------------------ projects

fn project_from_row(row: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        created_at: row.get(3)?,
    })
}

pub fn list_projects(conn: &Connection) -> AppResult<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, description, created_at FROM projects ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], project_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_project(conn: &Connection, id: i64) -> AppResult<Option<Project>> {
    Ok(conn
        .query_row(
            "SELECT id, name, description, created_at FROM projects WHERE id = ?1",
            [id],
            project_from_row,
        )
        .optional()?)
}

pub fn require_project(conn: &Connection, id: i64) -> AppResult<Project> {
    get_project(conn, id)?.ok_or_else(|| AppError::not_found("Project not found"))
}

pub fn create_project(conn: &Connection, name: &str, description: &str) -> AppResult<Project> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO projects (name, description, created_at) VALUES (?1, ?2, ?3)",
        params![name, description, now],
    )?;
    require_project(conn, conn.last_insert_rowid())
}

pub fn delete_project(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM projects WHERE id = ?1", [id])?;
    Ok(())
}

// ---------------------------------------------------------------- categories

fn category_from_row(row: &Row) -> rusqlite::Result<Category> {
    Ok(Category {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        created_at: row.get(4)?,
        paper_count: row.get(5)?,
    })
}

const CATEGORY_SELECT: &str = "SELECT c.id, c.project_id, c.name, c.description, c.created_at, \
     (SELECT COUNT(*) FROM papers p WHERE p.category_id = c.id) AS paper_count \
     FROM categories c";

pub fn list_categories(conn: &Connection, project_id: i64) -> AppResult<Vec<Category>> {
    let sql = format!("{CATEGORY_SELECT} WHERE c.project_id = ?1 ORDER BY c.created_at ASC, c.id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([project_id], category_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_category(conn: &Connection, id: i64) -> AppResult<Option<Category>> {
    let sql = format!("{CATEGORY_SELECT} WHERE c.id = ?1");
    Ok(conn.query_row(&sql, [id], category_from_row).optional()?)
}

pub fn category_name_exists(conn: &Connection, project_id: i64, name: &str) -> AppResult<bool> {
    let wanted = name.to_lowercase();
    let mut stmt = conn.prepare("SELECT name FROM categories WHERE project_id = ?1")?;
    let names = stmt.query_map([project_id], |r| r.get::<_, String>(0))?;
    for existing in names {
        if existing?.to_lowercase() == wanted {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn create_category(conn: &Connection, project_id: i64, name: &str, description: &str) -> AppResult<Category> {
    conn.execute(
        "INSERT INTO categories (project_id, name, description, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![project_id, name, description, now_iso()],
    )?;
    get_category(conn, conn.last_insert_rowid())?.ok_or_else(|| AppError::internal("Category vanished after insert"))
}

/// Delete a category. With `delete_papers` the category's papers (and their
/// annotations) go with it and their stored paths are returned so the caller can
/// remove the files; otherwise the papers become Uncategorized.
pub fn delete_category(conn: &mut Connection, id: i64, delete_papers: bool) -> AppResult<Vec<String>> {
    let tx = conn.transaction()?;
    let mut removed_paths = Vec::new();
    if delete_papers {
        {
            let mut stmt = tx.prepare("SELECT storage_path FROM papers WHERE category_id = ?1")?;
            let paths = stmt.query_map([id], |r| r.get::<_, String>(0))?;
            for path in paths {
                removed_paths.push(path?);
            }
        }
        tx.execute("DELETE FROM papers WHERE category_id = ?1", [id])?;
    } else {
        tx.execute("UPDATE papers SET category_id = NULL WHERE category_id = ?1", [id])?;
    }
    tx.execute("DELETE FROM categories WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(removed_paths)
}

// -------------------------------------------------------------------- papers

const PAPER_SELECT: &str = "SELECT p.id, p.project_id, p.category_id, p.title, p.original_filename, \
     p.citation_number, p.file_size, p.uploaded_at, COALESCE(c.name, 'Uncategorized'), \
     (SELECT COUNT(*) FROM annotations a WHERE a.paper_id = p.id), p.storage_path \
     FROM papers p LEFT JOIN categories c ON c.id = p.category_id";

fn paper_from_row(row: &Row) -> rusqlite::Result<PaperRecord> {
    Ok(PaperRecord {
        out: PaperOut {
            id: row.get(0)?,
            project_id: row.get(1)?,
            category_id: row.get(2)?,
            title: row.get(3)?,
            original_filename: row.get(4)?,
            citation_number: row.get(5)?,
            file_size: row.get(6)?,
            uploaded_at: row.get(7)?,
            category_name: row.get(8)?,
            annotation_count: row.get(9)?,
        },
        storage_path: row.get(10)?,
    })
}

pub fn list_papers(
    conn: &Connection,
    project_id: i64,
    category_id: Option<i64>,
    query: Option<&str>,
) -> AppResult<Vec<PaperOut>> {
    let sql = format!("{PAPER_SELECT} WHERE p.project_id = ?1 ORDER BY p.citation_number ASC, p.id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([project_id], paper_from_row)?;
    let needle = query.map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty());
    let mut out = Vec::new();
    for row in rows {
        let record = row?;
        if let Some(cat) = category_id {
            if record.out.category_id != Some(cat) {
                continue;
            }
        }
        if let Some(needle) = &needle {
            if !record.out.title.to_lowercase().contains(needle) {
                continue;
            }
        }
        out.push(record.out);
    }
    Ok(out)
}

pub fn get_paper(conn: &Connection, id: i64) -> AppResult<PaperRecord> {
    let sql = format!("{PAPER_SELECT} WHERE p.id = ?1");
    conn.query_row(&sql, [id], paper_from_row)
        .optional()?
        .ok_or_else(|| AppError::not_found("Research paper not found"))
}

pub struct NewPaper<'a> {
    pub project_id: i64,
    pub category_id: i64,
    pub title: &'a str,
    pub original_filename: &'a str,
    pub storage_path: &'a str,
    pub file_size: i64,
}

pub fn insert_paper(conn: &mut Connection, new: &NewPaper) -> AppResult<i64> {
    let tx = conn.transaction()?;
    let current_max: i64 = tx.query_row(
        "SELECT COALESCE(MAX(citation_number), 0) FROM papers WHERE project_id = ?1",
        [new.project_id],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO papers (project_id, category_id, title, original_filename, storage_path, \
         citation_number, file_size, uploaded_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            new.project_id,
            new.category_id,
            new.title,
            new.original_filename,
            new.storage_path,
            current_max + 1,
            new.file_size,
            now_iso()
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(id)
}

pub fn update_paper_title(conn: &Connection, id: i64, title: &str) -> AppResult<()> {
    conn.execute("UPDATE papers SET title = ?1 WHERE id = ?2", params![title, id])?;
    Ok(())
}

pub fn update_paper_category(conn: &Connection, id: i64, category_id: Option<i64>) -> AppResult<()> {
    conn.execute("UPDATE papers SET category_id = ?1 WHERE id = ?2", params![category_id, id])?;
    Ok(())
}

pub fn delete_paper(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM papers WHERE id = ?1", [id])?;
    Ok(())
}

// --------------------------------------------------------------- annotations

fn annotation_from_row(row: &Row) -> rusqlite::Result<Annotation> {
    let rects_text: String = row.get(6)?;
    Ok(Annotation {
        id: row.get(0)?,
        paper_id: row.get(1)?,
        page_number: row.get(2)?,
        annotation_type: row.get(3)?,
        text: row.get(4)?,
        color: row.get(5)?,
        rects: serde_json::from_str(&rects_text).unwrap_or(Value::Array(vec![])),
        created_at: row.get(7)?,
    })
}

const ANNOTATION_SELECT: &str =
    "SELECT id, paper_id, page_number, annotation_type, text, color, rects, created_at FROM annotations";

pub fn list_annotations(conn: &Connection, paper_id: i64) -> AppResult<Vec<Annotation>> {
    let sql = format!("{ANNOTATION_SELECT} WHERE paper_id = ?1 ORDER BY page_number, id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([paper_id], annotation_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn insert_annotation(
    conn: &Connection,
    paper_id: i64,
    page_number: i64,
    annotation_type: &str,
    text: &str,
    color: &str,
    rects: &[Value],
) -> AppResult<Annotation> {
    let rects_json = serde_json::to_string(rects)?;
    conn.execute(
        "INSERT INTO annotations (paper_id, page_number, annotation_type, text, color, rects, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![paper_id, page_number, annotation_type, text, color, rects_json, now_iso()],
    )?;
    let sql = format!("{ANNOTATION_SELECT} WHERE id = ?1");
    Ok(conn.query_row(&sql, [conn.last_insert_rowid()], annotation_from_row)?)
}

pub fn delete_annotation(conn: &Connection, id: i64) -> AppResult<bool> {
    Ok(conn.execute("DELETE FROM annotations WHERE id = ?1", [id])? > 0)
}

pub fn list_sentences(conn: &Connection, project_id: i64) -> AppResult<Vec<SentenceOut>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, p.id, p.citation_number, p.title, COALESCE(c.name, 'Uncategorized'), \
         a.page_number, a.text, a.color, a.created_at, a.rects \
         FROM annotations a \
         JOIN papers p ON a.paper_id = p.id \
         LEFT JOIN categories c ON p.category_id = c.id \
         WHERE p.project_id = ?1 AND a.annotation_type = 'sentence' \
         ORDER BY p.citation_number, a.created_at, a.id",
    )?;
    let rows = stmt.query_map([project_id], |row| {
        let rects_text: String = row.get(9)?;
        Ok(SentenceOut {
            id: row.get(0)?,
            paper_id: row.get(1)?,
            citation_number: row.get(2)?,
            paper_title: row.get(3)?,
            category_name: row.get(4)?,
            page_number: row.get(5)?,
            text: row.get(6)?,
            color: row.get(7)?,
            created_at: row.get(8)?,
            rects: serde_json::from_str(&rects_text).unwrap_or(Value::Array(vec![])),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// -------------------------------------------------------------------- images

const IMAGE_SELECT: &str = "SELECT i.id, i.project_id, i.paper_id, i.title, i.page_number, i.created_at, \
     p.title, c.name, i.storage_path \
     FROM important_images i \
     LEFT JOIN papers p ON p.id = i.paper_id \
     LEFT JOIN categories c ON c.id = p.category_id";

fn image_from_row(row: &Row) -> rusqlite::Result<ImageRecord> {
    let paper_id: Option<i64> = row.get(2)?;
    let paper_title: Option<String> = row.get(6)?;
    let category_name: Option<String> = row.get(7)?;
    // Same rule as the original: a linked paper without a category reads as
    // "Uncategorized"; an image with no paper has no category at all.
    let category_name = match (paper_id, category_name) {
        (_, Some(name)) => Some(name),
        (Some(_), None) => Some("Uncategorized".to_string()),
        (None, None) => None,
    };
    Ok(ImageRecord {
        out: ImageOut {
            id: row.get(0)?,
            project_id: row.get(1)?,
            paper_id,
            title: row.get(3)?,
            page_number: row.get(4)?,
            created_at: row.get(5)?,
            paper_title,
            category_name,
        },
        storage_path: row.get(8)?,
    })
}

pub fn list_images(conn: &Connection, project_id: i64) -> AppResult<Vec<ImageOut>> {
    let sql = format!("{IMAGE_SELECT} WHERE i.project_id = ?1 ORDER BY i.created_at DESC, i.id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([project_id], image_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?.out);
    }
    Ok(out)
}

pub fn get_image(conn: &Connection, id: i64) -> AppResult<ImageRecord> {
    let sql = format!("{IMAGE_SELECT} WHERE i.id = ?1");
    conn.query_row(&sql, [id], image_from_row)
        .optional()?
        .ok_or_else(|| AppError::not_found("Image clipping not found"))
}

pub fn insert_image(
    conn: &Connection,
    project_id: i64,
    paper_id: Option<i64>,
    title: &str,
    page_number: Option<i64>,
    storage_path: &str,
) -> AppResult<ImageOut> {
    conn.execute(
        "INSERT INTO important_images (project_id, paper_id, title, page_number, storage_path, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![project_id, paper_id, title, page_number, storage_path, now_iso()],
    )?;
    Ok(get_image(conn, conn.last_insert_rowid())?.out)
}

pub fn delete_image(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM important_images WHERE id = ?1", [id])?;
    Ok(())
}

// --------------------------------------------------------------------- latex

const LATEX_SELECT: &str =
    "SELECT id, project_id, title, created_at, updated_at, compiled, last_compile_log, storage_dir FROM latex_documents";

fn latex_from_row(row: &Row) -> rusqlite::Result<LatexRecord> {
    Ok(LatexRecord {
        out: LatexOut {
            id: row.get(0)?,
            project_id: row.get(1)?,
            title: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            compiled: row.get::<_, i64>(5)? != 0,
            last_compile_log: row.get(6)?,
        },
        storage_dir: row.get(7)?,
    })
}

pub fn list_latex(conn: &Connection, project_id: i64) -> AppResult<Vec<LatexOut>> {
    let sql = format!("{LATEX_SELECT} WHERE project_id = ?1 ORDER BY updated_at DESC, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([project_id], latex_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?.out);
    }
    Ok(out)
}

pub fn get_latex(conn: &Connection, id: i64) -> AppResult<LatexRecord> {
    let sql = format!("{LATEX_SELECT} WHERE id = ?1");
    conn.query_row(&sql, [id], latex_from_row)
        .optional()?
        .ok_or_else(|| AppError::not_found("LaTeX document not found"))
}

pub fn insert_latex(conn: &Connection, project_id: i64, title: &str, storage_dir: &str) -> AppResult<LatexRecord> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO latex_documents (project_id, title, storage_dir, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![project_id, title, storage_dir, now],
    )?;
    get_latex(conn, conn.last_insert_rowid())
}

pub fn touch_latex(conn: &Connection, id: i64) -> AppResult<String> {
    let now = now_iso();
    conn.execute("UPDATE latex_documents SET updated_at = ?1 WHERE id = ?2", params![now, id])?;
    Ok(now)
}

pub fn set_latex_build(conn: &Connection, id: i64, compiled: bool, log: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE latex_documents SET compiled = ?1, last_compile_log = ?2 WHERE id = ?3",
        params![compiled as i64, log, id],
    )?;
    Ok(())
}

pub fn delete_latex(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM latex_documents WHERE id = ?1", [id])?;
    Ok(())
}

/// Storage paths of everything a project owns, collected before deletion so the
/// files can be removed afterwards.
pub struct ProjectFiles {
    pub papers: Vec<String>,
    pub images: Vec<String>,
    pub latex_dirs: Vec<String>,
}

pub fn project_files(conn: &Connection, project_id: i64) -> AppResult<ProjectFiles> {
    fn column(conn: &Connection, sql: &str, id: i64) -> AppResult<Vec<String>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    Ok(ProjectFiles {
        papers: column(conn, "SELECT storage_path FROM papers WHERE project_id = ?1", project_id)?,
        images: column(conn, "SELECT storage_path FROM important_images WHERE project_id = ?1", project_id)?,
        latex_dirs: column(conn, "SELECT storage_dir FROM latex_documents WHERE project_id = ?1", project_id)?,
    })
}
