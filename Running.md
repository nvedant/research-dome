# Research Dome

Desktop research library: organise papers by project and category, read PDFs, save important sentences and figures, export annotated PDFs, and write papers in LaTeX. Rust (axum + SQLite) backend inside a Tauri v2 shell, React frontend. Fully offline; all data stays on the user's computer.

## Features worth knowing
- **PDF tabs:** open several papers at once. Use the `+` in the reader to open another paper, click tabs to switch, middle-click or `Ctrl+W` to close, `Ctrl+Tab` to cycle. "Library" goes back to your papers while keeping the tabs open; they are restored next time you open the project.
- **LaTeX suggestions (offline):** typing `\cite{` suggests keys from your `.bib` files (with author/year), `\includegraphics{` suggests uploaded images/PDFs, `\ref{` suggests your `\label`s, `\input{`/`\include{` suggest `.tex` files, and `\bibliography{` suggests `.bib` files.

## Requirements (to build)
- Rust (stable): https://rustup.rs
- Node.js 22+
- Windows: Visual Studio Build Tools ("Desktop development with C++"); WebView2 (preinstalled on Windows 10/11)
- Linux: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libssl-dev patchelf build-essential`
- macOS: `xcode-select --install`

Optional at runtime (only for "Build PDF" in LaTeX Studio): MiKTeX / TeX Live / MacTeX.

## Commands
```
npm install
npm run app:dev      # run in development
npm run app:build    # build installers
npm run test:core    # backend integration tests
```
Installers appear in `target/release/bundle/`:
- Windows: `nsis/Research Dome_1.0.0_x64-setup.exe`, `msi/*.msi`
- macOS: `dmg/*.dmg`
- Linux: `deb/`, `appimage/`, `rpm/`

Windows installers must be built on Windows, macOS on macOS, Linux on Linux. Push a `v*` tag to GitHub to build all three automatically (`.github/workflows/release.yml`).

## App icon
Replace `app-icon.png` in the project root (square PNG, 1024×1024 recommended), then run:
```
npm run icon
```
This regenerates everything in `src-tauri/icons/`. For the in-app logo also replace `src/assets/icon-512.png`, `src/assets/icon-160.png` and `public/favicon.png`.

## Data location
Windows `%APPDATA%\com.researchdome.app`, macOS `~/Library/Application Support/com.researchdome.app`, Linux `~/.local/share/com.researchdome.app`. Override with `RESEARCH_DOME_DATA_DIR`. Contains `research-dome.sqlite`, `storage/` (PDFs, figures, LaTeX projects) and `logs/`.

## Headless API server (dev/testing)
`npm run server` starts the same API on `DOME_PORT` (default 8000) with token `DOME_TOKEN` (default `dev-token`).
