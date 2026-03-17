# SeWriter

A minimal writing app that stays out of your way.

SeWriter lives in the background and appears instantly with a global hotkey. 

Open - Write - Close. That's it.

---

## Download

**[→ Download SeWriter for Mac](https://github.com/sego443/SeWriter/releases/tag/v0.1.0)** (macOS 12+, Apple Silicon & Intel)

---

## How it works

Press **ctrl+W** from anywhere to show the window. SeWriter is always running in the background. 

Press **cmd+W** to close the window.

Press **cmd+S** to save and close the window. 

Press **cmd+Q** to quit. 

Every session has a **title** and a **body**. 

SeWriter saves your work automatically as you type.

---

## Commands

Press **Cmd+/** while writing to open the command panel. Type to filter, arrow keys to navigate, Enter to confirm, Esc to cancel.

| Command | What it does |
|---|---|
| `/new` | Save the current file and start a new one |
| `/finish` | Save and close; next time opens a blank title |
| `/title` | Rename the current file's title |
| `/re` | Browse and reopen a previous file from the vault |
| `/vault new` | Choose a new folder as your vault |
| `/vault reset` | Relocate to a different vault |
| `/config` | Adjust settings (font size, etc.) |

---

## The vault

All your files are saved as plain `.txt` files in a folder of your choice — your **vault**. Each save is versioned (`title-1.txt`, `title-2.txt`, …) and a live draft is always kept as `title-tmp.txt`.

---

## Built with

Rust · [egui](https://github.com/emilk/egui) · Metal (wgpu)
