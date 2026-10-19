//! LaTeX Studio back end: workspace file rules and the PDF build pipeline
//! (pdfLaTeX → Biber/BibTeX when needed → follow-up passes), ported one-to-one
//! from the original service.
//!
//! A TeX distribution (MiKTeX, MacTeX/TeX Live, TinyTeX …) must be installed on
//! the machine for *Build PDF*. Desktop apps launched from the Start menu or
//! Finder do not inherit a shell `PATH`, so well-known install locations are
//! searched as well.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use regex::Regex;
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::util::{extension_lower, tail_chars};

pub const TEXT_EXTENSIONS: &[&str] = &[
    ".tex", ".bib", ".sty", ".cls", ".bst", ".def", ".cfg", ".tikz", ".pgf", ".csv", ".dat", ".txt",
];
pub const IMAGE_EXTENSIONS: &[&str] = &[".png", ".jpg", ".jpeg", ".webp", ".svg", ".eps"];
pub const GENERATED_NAMES: &[&str] = &[
    "main.pdf", "main.aux", "main.log", "main.out", "main.toc", "main.fls", "main.fdb_latexmk",
    "main.bbl", "main.blg", "main.bcf", "main.run.xml", "main.synctex.gz",
];
pub const MAX_ASSET_BYTES: usize = 30 * 1024 * 1024;
const LOG_LIMIT: usize = 20_000;
const PASS_TIMEOUT: Duration = Duration::from_secs(90);
const LATEXMK_TIMEOUT: Duration = Duration::from_secs(120);

pub const STARTER_SOURCE: &str = r"\documentclass[12pt]{article}
\usepackage[margin=1in]{geometry}
\usepackage{graphicx}
\usepackage{amsmath,amssymb}
\usepackage{hyperref}
\title{Research Paper Title}
\author{Author Name}
\date{\today}
\begin{document}
\maketitle
\begin{abstract}
Write your abstract here.
\end{abstract}
\section{Introduction}
Start writing your research paper in LaTeX. Save or compile to preview the PDF.
\section{Related Work}
Summarize relevant research and cite sources using BibTeX or thebibliography.
\section{Conclusion}
Summarize your findings and future work.
\end{document}
";

pub fn is_text_ext(ext: &str) -> bool {
    TEXT_EXTENSIONS.contains(&ext)
}

pub fn is_allowed_ext(ext: &str) -> bool {
    is_text_ext(ext) || IMAGE_EXTENSIONS.contains(&ext) || ext == ".pdf"
}

pub fn file_kind(ext: &str) -> &'static str {
    if is_text_ext(ext) {
        "text"
    } else if IMAGE_EXTENSIONS.contains(&ext) {
        "image"
    } else {
        "pdf"
    }
}

pub fn is_generated(name: &str) -> bool {
    let lower = name.to_lowercase();
    GENERATED_NAMES.contains(&lower.as_str())
}

/// Resolve a workspace-relative file path while preventing path traversal.
pub fn workspace_file(root: &Path, relative: &str, for_write: bool) -> AppResult<PathBuf> {
    let raw = relative.replace('\\', "/");
    let raw = raw.trim();
    let invalid = || {
        AppError::bad_request("Use a valid relative filename, such as references.bib or figures/chart.png")
    };
    if raw.is_empty() || raw.starts_with('/') {
        return Err(invalid());
    }
    let parts: Vec<&str> = raw.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains(':') || p.contains('\0'))
    {
        return Err(invalid());
    }
    let mut target = root.to_path_buf();
    for part in &parts {
        target.push(part);
    }
    let name = parts.last().copied().unwrap_or("");
    let ext = extension_lower(name);
    if !is_allowed_ext(&ext) {
        let shown = if ext.is_empty() { "(no extension)".to_string() } else { ext };
        return Err(AppError::bad_request(format!("Unsupported LaTeX project file type: {shown}")));
    }
    if for_write && is_generated(name) {
        return Err(AppError::bad_request("Compiler output files cannot be edited directly"));
    }
    Ok(target)
}

#[derive(Debug, Serialize)]
pub struct WorkspaceFile {
    pub path: String,
    pub name: String,
    pub extension: String,
    pub kind: &'static str,
    pub size: u64,
}

pub fn list_files(root: &Path) -> Vec<WorkspaceFile> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<WorkspaceFile>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                walk(&path, root, out);
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let ext = extension_lower(&name);
            if !is_allowed_ext(&ext) || is_generated(&name) {
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else { continue };
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push(WorkspaceFile {
                path: rel,
                name,
                kind: file_kind(&ext),
                extension: ext,
                size: meta.len(),
            });
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort_by(|a, b| {
        (a.path != "main.tex", a.path.to_lowercase()).cmp(&(b.path != "main.tex", b.path.to_lowercase()))
    });
    files
}

pub fn content_type_for(ext: &str) -> &'static str {
    match ext {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".webp" => "image/webp",
        ".svg" => "image/svg+xml",
        ".eps" => "application/postscript",
        ".pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

// ------------------------------------------------------------ tool discovery

#[cfg(windows)]
const EXE_SUFFIX: &str = ".exe";
#[cfg(not(windows))]
const EXE_SUFFIX: &str = "";

/// `<parent>/<child>/<tail...>` for every child directory of `parent`.
fn child_dirs(parent: &Path, tail: &[&str]) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(parent) else { return Vec::new() };
    entries
        .flatten()
        .map(|entry| {
            let mut path = entry.path();
            for part in tail {
                path.push(part);
            }
            path
        })
        .collect()
}

fn extra_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    #[cfg(unix)]
    {
        // TeX Live: /usr/local/texlive/<year>/bin/<arch>
        for year in child_dirs(Path::new("/usr/local/texlive"), &["bin"]) {
            dirs.extend(child_dirs(&year, &[]));
        }
        for fixed in [
            "/Library/TeX/texbin",
            "/usr/texbin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
        ] {
            dirs.push(PathBuf::from(fixed));
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            for parent in [home.join(".TinyTeX/bin"), home.join("Library/TinyTeX/bin")] {
                dirs.extend(child_dirs(&parent, &[]));
            }
        }
    }

    #[cfg(windows)]
    {
        for var in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                dirs.push(base.join(r"Programs\MiKTeX\miktex\bin\x64"));
                dirs.push(base.join(r"MiKTeX\miktex\bin\x64"));
                dirs.push(base.join(r"MiKTeX\miktex\bin"));
            }
        }
        // TeX Live: C:\texlive\<year>\bin\windows (or win64)
        dirs.extend(child_dirs(Path::new(r"C:\texlive"), &["bin", "windows"]));
        dirs.extend(child_dirs(Path::new(r"C:\texlive"), &["bin", "win64"]));
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join(r"TinyTeX\bin\windows"));
        }
    }

    dirs
}

/// Find an executable on `PATH` or in common TeX install locations.
pub fn find_executable(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{EXE_SUFFIX}");
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(extra_search_dirs());
    dirs.into_iter().map(|d| d.join(&file)).find(|candidate| candidate.is_file())
}

// ----------------------------------------------------------------- build

#[derive(Debug, Clone)]
pub struct CompileOutcome {
    pub ok: bool,
    pub log: String,
}

enum RunError {
    Timeout,
    Spawn(String),
}

struct RunOutput {
    success: bool,
    text: String,
}

async fn run_tool(exe: &Path, args: &[&str], cwd: &Path, limit: Duration) -> Result<RunOutput, RunError> {
    let mut command = tokio::process::Command::new(exe);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Helper programs (bibtex, kpathsea …) are resolved relative to the TeX bin dir.
    if let Some(bin_dir) = exe.parent() {
        let mut paths = vec![bin_dir.to_path_buf()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            command.env("PATH", joined);
        }
    }
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash on Windows

    let child = command.spawn().map_err(|e| RunError::Spawn(e.to_string()))?;
    match tokio::time::timeout(limit, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(RunOutput {
                success: output.status.success(),
                text: format!("{stdout}\n{stderr}").trim().to_string(),
            })
        }
        Ok(Err(e)) => Err(RunError::Spawn(e.to_string())),
        Err(_) => Err(RunError::Timeout),
    }
}

fn outcome_fail(logs: &[String]) -> CompileOutcome {
    CompileOutcome { ok: false, log: tail_chars(&logs.join("\n\n"), LOG_LIMIT) }
}

fn spawn_fail(logs: &mut Vec<String>, tool: &str, err: &str) -> CompileOutcome {
    logs.push(format!("[{tool}] Could not start the program: {err}"));
    outcome_fail(logs)
}

fn timeout_fail(logs: &mut Vec<String>) -> CompileOutcome {
    logs.push("Compilation timed out. Check the compiler log and document assets.".to_string());
    outcome_fail(logs)
}

pub async fn compile(work_dir: &Path) -> CompileOutcome {
    if let Err(err) = std::fs::create_dir_all(work_dir) {
        return CompileOutcome { ok: false, log: format!("Could not create the workspace folder: {err}") };
    }
    let source = work_dir.join("main.tex");
    if !source.exists() {
        let _ = std::fs::write(&source, "");
    }

    let pdflatex = find_executable("pdflatex");
    let latexmk = find_executable("latexmk");
    if pdflatex.is_none() && latexmk.is_none() {
        return CompileOutcome {
            ok: false,
            log: "No LaTeX compiler was found. Install MiKTeX (Windows), MacTeX (macOS) or TeX Live (Linux), \
                  make sure pdflatex is available, then restart Research Dome and build again."
                .to_string(),
        };
    }

    // Remove stale auxiliaries so switching between BibTeX and biblatex/Biber is
    // never affected by an earlier build. The last PDF is kept until a new build
    // succeeds so the preview survives a failed compile.
    for suffix in [
        ".aux", ".bbl", ".blg", ".bcf", ".run.xml", ".out", ".toc", ".lof", ".lot", ".fls", ".fdb_latexmk",
        ".synctex.gz",
    ] {
        let _ = std::fs::remove_file(work_dir.join(format!("main{suffix}")));
    }

    let mut logs: Vec<String> = Vec::new();
    let pdf_path = work_dir.join("main.pdf");

    if let Some(pdflatex) = pdflatex {
        let args = ["-interaction=nonstopmode", "-halt-on-error", "-file-line-error", "main.tex"];

        macro_rules! pass {
            () => {
                match run_tool(&pdflatex, &args, work_dir, PASS_TIMEOUT).await {
                    Ok(result) => {
                        logs.push(format!("[pdflatex]\n{}", result.text));
                        result.success
                    }
                    Err(RunError::Timeout) => return timeout_fail(&mut logs),
                    Err(RunError::Spawn(err)) => return spawn_fail(&mut logs, "pdflatex", &err),
                }
            };
        }

        if !pass!() {
            return outcome_fail(&logs);
        }

        let aux_text = std::fs::read(work_dir.join("main.aux"))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let mut bibliography_ran = false;

        // biblatex writes main.bcf and needs Biber; classic BibTeX writes \bibdata into main.aux.
        if work_dir.join("main.bcf").exists() {
            match find_executable("biber") {
                Some(biber) => match run_tool(&biber, &["main"], work_dir, PASS_TIMEOUT).await {
                    Ok(result) => {
                        logs.push(format!("[biber]\n{}", result.text));
                        if !result.success {
                            return outcome_fail(&logs);
                        }
                        bibliography_ran = true;
                    }
                    Err(RunError::Timeout) => return timeout_fail(&mut logs),
                    Err(RunError::Spawn(err)) => return spawn_fail(&mut logs, "biber", &err),
                },
                None => {
                    logs.push(
                        "[bibliography error] This document uses biblatex and requires Biber, but biber was not \
                         found. Install the biber package with your TeX distribution (MiKTeX Console on Windows), \
                         restart Research Dome and build again."
                            .to_string(),
                    );
                    return outcome_fail(&logs);
                }
            }
        } else {
            let bibdata = Regex::new(r"\\bibdata\s*\{").expect("static regex");
            let citation = Regex::new(r"\\citation\s*\{").expect("static regex");
            if bibdata.is_match(&aux_text) && citation.is_match(&aux_text) {
                match find_executable("bibtex") {
                    Some(bibtex) => match run_tool(&bibtex, &["main"], work_dir, PASS_TIMEOUT).await {
                        Ok(result) => {
                            logs.push(format!("[bibtex]\n{}", result.text));
                            if !result.success {
                                return outcome_fail(&logs);
                            }
                            bibliography_ran = true;
                        }
                        Err(RunError::Timeout) => return timeout_fail(&mut logs),
                        Err(RunError::Spawn(err)) => return spawn_fail(&mut logs, "bibtex", &err),
                    },
                    None => {
                        logs.push(
                            "[bibliography error] A BibTeX bibliography was detected, but bibtex was not found. \
                             Install the BibTeX package with your TeX distribution, restart Research Dome and \
                             build again."
                                .to_string(),
                        );
                        return outcome_fail(&logs);
                    }
                }
            }
        }

        // A bibliography needs two follow-up passes (citations + reference list);
        // an ordinary document needs one more for cross-references.
        let follow_up = if bibliography_ran { 2 } else { 1 };
        for _ in 0..follow_up {
            if !pass!() {
                return outcome_fail(&logs);
            }
        }
    } else if let Some(latexmk) = latexmk {
        let args = ["-pdf", "-interaction=nonstopmode", "-halt-on-error", "main.tex"];
        match run_tool(&latexmk, &args, work_dir, LATEXMK_TIMEOUT).await {
            Ok(result) => {
                logs.push(format!("[latexmk]\n{}", result.text));
                if !result.success {
                    return outcome_fail(&logs);
                }
            }
            Err(RunError::Timeout) => return timeout_fail(&mut logs),
            Err(RunError::Spawn(err)) => return spawn_fail(&mut logs, "latexmk", &err),
        }
    }

    CompileOutcome { ok: pdf_path.exists(), log: tail_chars(&logs.join("\n\n"), LOG_LIMIT) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths_are_validated() {
        let root = Path::new("/ws");
        assert!(workspace_file(root, "main.tex", false).is_ok());
        assert!(workspace_file(root, "figures/plot.png", true).is_ok());
        assert!(workspace_file(root, "../x.tex", false).is_err());
        assert!(workspace_file(root, "/abs.tex", false).is_err());
        assert!(workspace_file(root, "a//b.tex", false).is_err());
        assert!(workspace_file(root, "evil.exe", false).is_err());
        assert!(workspace_file(root, "main.pdf", true).is_err());
        assert!(workspace_file(root, "main.pdf", false).is_ok());
        assert!(workspace_file(root, "C:/x.tex", false).is_err());
    }

    #[test]
    fn kinds() {
        assert_eq!(file_kind(".bib"), "text");
        assert_eq!(file_kind(".png"), "image");
        assert_eq!(file_kind(".pdf"), "pdf");
    }
}
