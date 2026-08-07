# Brand Assets — Quick Regeneration

After artwork changes to `brand/source/brand-sheet.png`, run:

```bash
python3 brand/make-assets.py
cd desktop/src-tauri && cargo tauri icon ../../brand/app-icon.png
```

This outputs:
- `brand/app-icon.png` — 1024×1024 app icon (Tauri source)
- `brand/mark.png` — 512×512 circular badge
- `brand/lockup.png` — 1200×~470 horizontal wordmark
- `web/public/momo-mark-64.png` — header icon
- `web/public/favicon.png` — favicon

The script is pure Python stdlib, so it needs no new dependencies. For details on what it does and why, see [code-guide.md](code-guide.md) and [make-assets-guide.md](make-assets-guide.md).

**Workflow note:** The script crops three regions from the source sheet at fixed coordinates, and writes five files from them. If the layout of `brand-sheet.png` changes, you will need to update `ICON_BOX`, `FAVICON_BOX`, and `LOCKUP_BOX` in the script — see the coordinate reference in [make-assets-guide.md](make-assets-guide.md).
