<p align="center">
  <img src="./crates/winc-app/assets/icon.svg" alt="obnote" width="128" />
</p>

<h1 align="center">obnote</h1>

<p align="center">
  Ultra-lightweight, pure-Rust local knowledge base &amp; office document manager (Obsidian-lite).
</p>

<p align="center">
  English | <a href="./README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="./LICENSE"><img alt="License" src="https://img.shields.io/badge/License-Apache%202.0-blue" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2024-b7410e" />
  <img alt="Platforms" src="https://img.shields.io/badge/Platforms-macOS%20%7C%20Linux%20%7C%20Windows-555" />
</p>

## Vision & Philosophy

`obnote` is built for individuals and teams who manage **local Markdown notes alongside everyday office documents**:

- **Zero Electron Bloat**: Unlike Electron/WebView note apps that routinely consume hundreds of megabytes of RAM, `obnote` is written in pure Rust with a custom CPU rasterizer—starting in **~119 ms** and idling around **~50 MB RAM** (52 MB with 100,000 lines open).
- **Focused & Practical**: No heavy force-directed graph view or sprawling plugin runtime. `obnote` focuses on three essentials: **pleasant Markdown writing, instant search across 200k+ files, and built-in preview for real-world office formats**.
- **Clean Out of the Box**: Developer-centric modules (Git, integrated terminal, SSH remote, Vim mode) can be toggled off so the workspace stays calm and distraction-free.

## Features & Format Matrix

| Area | Capabilities |
| --- | --- |
| **Markdown Notes** | Live Preview (source expands at cursor), LaTeX math, 9 native Mermaid diagram types, `[[wikilinks]]`, automatic clipboard image saving (`assets/${filename}`), outline / TOC, focus mode, and typewriter mode |
| **Office & Document Previews** | Word (`.docx`), Excel / spreadsheets (`.xlsx`, `.csv`, `.tsv`), PowerPoint (`.pptx` slide-by-slide preview in pure Rust with zero new dependencies), HTML Reader Mode (< 5 MB RAM + one-click "Open in Browser"), PDF (macOS / Windows bitmap rendering + outline + thumbnails), EPUB, SVG, and common image formats |
| **Search & External Handoff** | Fast project-wide full-text search (natively indexing `.docx` and `.pptx` text content), instant Quick Open across 200k+ files, and a one-click **"Open with default app"** toolbar button for external PDF annotation or full Office editing |
| **Interface** | 16 built-in light and dark themes, complete English and Simplified Chinese UI, touchscreen & trackpad pinch-to-zoom |

## Upstream Attribution

`obnote` is built on top of **[Winc](https://github.com/DEEIX-AI/Winc)** (Apache-2.0).

Winc provides the pure-Rust desktop shell and custom CPU rasterizer (writing directly to `IOSurface` on macOS) that power the ~119 ms cold start, ~50 MB memory footprint, and smooth scrolling in huge files.

To track upstream performance and bug fixes cleanly, `obnote` uses a **Thin Rebrand** strategy:
- Dedicated `obnote` binary, custom note icon, isolated single-instance lock, and `obnote://` URL scheme;
- Isolated config and state directories so `obnote` and upstream `winc` can coexist side by side;
- Preserved internal `winc-*` crate layout for straightforward upstream merges.

## Quick Start

### Build & Run from Source

Requires Rust (`rust-toolchain.toml` selects `1.99.1` automatically):

```bash
# Build and open your local vault / document folder
cargo run --release --bin obnote -- <path-to-your-vault>

# Or build first and invoke the release binary directly
cargo build --release --bin obnote
./target/release/obnote <path-to-your-vault>
```

### Configuration Paths

`obnote` stores its configuration separately from upstream `winc`:

| Platform | Config Path |
| --- | --- |
| **macOS** | `~/Library/Application Support/obnote/config.json` |
| **Linux** | `~/.config/obnote/config.json` |
| **Windows** | `%APPDATA%\obnote\config.json` |

### Sample "Clean Vault Mode" Config

Add the following to `config.json` to disable developer modules (Git, terminal, SSH remote, Vim) and enable auto-save, outline, and per-note clipboard image folders:

```json
{
  "language": "en",
  "theme": "auto",
  "editor": {
    "defaultLanguage": "markdown",
    "autoSave": "onFocusChange",
    "restoreFolder": true,
    "wordWrap": true,
    "outline": true,
    "lineNumbers": false
  },
  "markdown": {
    "textWidth": "standard",
    "imageFolder": "assets/${filename}",
    "imageName": "${date}_${filename}"
  },
  "git": { "enabled": false },
  "terminal": { "enabled": false },
  "remote": { "enabled": false },
  "vim": { "enabled": false }
}
```

## Roadmap

- **Stage 1 (Completed)**:
  - [x] Thin Rebrand (`obnote` binary, isolated config/state paths, single-instance lock, and custom folded-note icon)
  - [x] One-click **"Open with default app"** preview toolbar button
  - [x] Lightweight HTML Reader Mode (< 5 MB RAM + "Open in Browser")
  - [x] Pure-Rust PowerPoint (`.pptx`) slide-by-slide preview
  - [x] Native `.docx` and `.pptx` text indexing in project-wide search
- **Stage 2 (Planned)**:
  - [ ] Backlinks panel
  - [ ] `#tags` and YAML Frontmatter index
  - [ ] Auto-update `[[wikilinks]]` on file rename
  - [ ] PDF text selection and copy

## Community & Acknowledgments

- Upstream project: [DEEIX AI / Winc](https://github.com/DEEIX-AI/Winc)
- Open-source community: [LINUX DO](https://linux.do)

## License

`obnote` is licensed under the [Apache License, Version 2.0](LICENSE), preserving the original [LICENSE](LICENSE) and [NOTICE](NOTICE) attribution to [DEEIX AI / Winc](https://github.com/DEEIX-AI/Winc).
