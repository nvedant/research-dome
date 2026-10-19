# Publishing releases on GitHub

This guide shows how to put the Windows (`.exe`, `.msi`), macOS (`.dmg`) and Linux (`.deb`, `.AppImage`, `.rpm`) installers in your repository's **Releases** section so anyone can download them.

An installer can only be built on its own operating system. You have two options:

- **Option A — Automatic (recommended):** GitHub builds all three for you.
- **Option B — Manual:** you build on each computer and upload the files yourself.

---

## One-time setup: put the project on GitHub

1. Create a new repository on <https://github.com/new> (for example `research-dome`).
2. In the project folder:
   ```bash
   git init
   git add .
   git commit -m "Research Dome 1.0.0"
   git branch -M main
   git remote add origin https://github.com/YOUR-USERNAME/research-dome.git
   git push -u origin main
   ```
3. Replace every `YOUR-USERNAME` in `README.md` with your GitHub username.

`.gitignore` already excludes `node_modules`, `target` and `dist`.

---

## Option A — Automatic release (GitHub Actions)

The workflow in `.github/workflows/release.yml` builds Windows, macOS and Linux in the cloud and attaches every installer to a **draft** release.

### 1. Allow the workflow to create releases (once)
Repository → **Settings → Actions → General → Workflow permissions** → select **Read and write permissions** → **Save**.

### 2. Set the version
The version must be the same in these three files:

| File | Field |
|---|---|
| `src-tauri/tauri.conf.json` | `"version": "1.0.0"` |
| `package.json` | `"version": "1.0.0"` |
| `Cargo.toml` (top-level) | `version = "1.0.0"` under `[workspace.package]` |

### 3. Push a version tag
```bash
git add .
git commit -m "Release 1.0.0"
git push
git tag v1.0.0
git push origin v1.0.0
```
The tag must start with `v` and match the version (`v1.0.0` for `1.0.0`).

### 4. Wait for the build
Open the **Actions** tab. The *Release* workflow takes roughly 10–25 minutes the first time (later runs are faster because of caching). Three jobs run: `windows-latest`, `macos-latest`, `ubuntu-22.04`.

### 5. Publish the release
Go to the **Releases** page → you will see a **Draft** called *Research Dome v1.0.0* with all the installers attached. Edit the notes if you like, then click **Publish release**. Anyone can now download from it.

You can also start a build without a tag: **Actions → Release → Run workflow**.

### Re-running a failed build
Actions → open the failed run → **Re-run failed jobs**.

---

## Option B — Manual release

### 1. Build on each system
Run on a computer with that operating system:
```bash
npm install
npm run app:build
```
Files appear in `target/release/bundle/`:

| OS | Upload these files |
|---|---|
| Windows | `nsis/Research Dome_1.0.0_x64-setup.exe` and `msi/Research Dome_1.0.0_x64_en-US.msi` |
| macOS | `dmg/Research Dome_1.0.0_*.dmg` |
| Linux | `deb/*.deb`, `appimage/*.AppImage`, `rpm/*.rpm` |

(Exact names can differ slightly by version and CPU.)

### 2. Create the release on GitHub
1. Open your repository → **Releases** (right sidebar) → **Draft a new release**.
2. **Choose a tag** → type `v1.0.0` → **Create new tag: v1.0.0 on publish**.
3. **Release title:** `Research Dome v1.0.0`.
4. Describe what's new (a short bullet list is perfect).
5. **Attach binaries:** drag all the installer files into the box at the bottom (or click *"Attach binaries by dropping them here"*). Wait until each file shows as uploaded.
6. Tick **Set as the latest release** and click **Publish release**.

Your download link is now `https://github.com/YOUR-USERNAME/research-dome/releases/latest`.

---

## Publishing an update later

1. Make your changes and test with `npm run app:dev`.
2. Raise the version in the three files above (for example to `1.1.0`).
3. Commit, push, then `git tag v1.1.0 && git push origin v1.1.0` (Option A), or repeat the manual steps (Option B).

Users keep their data when they install the newer version over the old one.

---

## Good to know

- **Code signing:** the installers are unsigned, so Windows SmartScreen and macOS Gatekeeper show a first-run warning (explained in the README). A Windows code-signing certificate and an Apple Developer account remove those warnings; they are optional and cost money.
- **File size limit:** GitHub allows up to 2 GB per release file.
- **Checksums (optional):** to let people verify downloads, run `certutil -hashfile "Research Dome_1.0.0_x64-setup.exe" SHA256` (Windows) or `shasum -a 256 file` (macOS/Linux) and paste the result in the release notes.
- **Private repository:** release downloads need a GitHub login. Make the repository public if you want everyone to download.
