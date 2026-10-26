# ReBook

A quiet reader for macOS in the style of ReNote: PDF, EPUB and comics (CBZ, CBR).
Rust (Tauri) keeps the library; the interface is Svelte.

- **Spreads**: portrait pages are shown two per screen, a landscape page takes the
  whole screen; the cover stands alone. EPUB text reflows in two columns on wide windows.
- **Library**: books are copied in on import (ADD or drop files onto the window),
  with title, author, cover and the place you stopped reading.
- Keys: ↑ ↓ select · Enter read · ← → / Space turn pages · F full screen · Esc back ·
  / search · A add · I info · Backspace remove

## Run

```sh
npm install
npm run desktop        # dev window with live reload
npm run desktop:build  # ReBook.app + .dmg in src-tauri/target/release/bundle/
```

Rust tests: `cd src-tauri && cargo test`.

The library lives in `~/Library/Application Support/com.artursavytskyi.rebook/`.
