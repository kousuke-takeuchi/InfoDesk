**日本語: [11-keyboard-shortcuts.ja.md](11-keyboard-shortcuts.ja.md)** | ← [Roadmap](10-roadmap.md)

# Keyboard Shortcuts

InfoDesk can be driven from the keyboard. Press `?` anywhere to open the list inside the app, and press `Esc` to close it. On macOS the modifier is Command; on other keyboards it is Ctrl.

## 1. Global shortcuts

| Keys | Action |
|---|---|
| `Cmd+R` / `Ctrl+R` | Collect now, and refresh the view |
| `1` / `2` | Digest view / Feed view |
| `d` / `w` / `m` | Period: today / week / month (in the feed, the last 1 week and 1 month) |
| `[` / `]` | Previous / next item in the sidebar (wraps around at the ends) |
| `0` | Select "All" in the sidebar |
| `Cmd+J` / `Ctrl+J` | Open or close the chat panel |
| `Cmd+,` / `Ctrl+,` | Open Settings |
| `?` | Open the shortcut list |

The sidebar order is All, then the categories, then your topics.

## 2. Feed shortcuts

These work in the Feed view only.

| Keys | Action |
|---|---|
| `j` / `k` | Next / previous article |
| `Enter` | Open the selected article in the reader |
| `u` | Toggle read and unread |
| `s` | Toggle the star |
| `/` | Focus the search box |
| `Shift+A` | Mark all articles matching the current filters as read |

In the Digest view, only the global shortcuts apply (including `d`, `w` and `m` for the period).

## 3. When shortcuts are ignored

- **While typing**: nothing fires when focus is in a text field, a text area, a select or an editable area (the search box, the chat input, Settings fields).
- **During IME composition**: keys pressed while composing text (for example Japanese input) are ignored.
- **When a dialog is open**: a modal dialog, including the shortcut list itself, blocks every shortcut.
- **In Settings**: only `Cmd+R`, `Cmd+,`, `Cmd+J` and `?` work. The other keys are ignored so they cannot change a setting by accident.
- **On a button, link or summary**: `Enter` keeps its normal meaning (pressing the control) instead of opening the selected article.
- **With extra modifiers**: plain keys do nothing when Ctrl, Command or Alt is held (`Shift` is only used for `?` and `Shift+A`).

## 4. Fresh data

The window reloads its data when it regains focus or becomes visible again, and when the local date changes (checked once a minute). The digest and the feed therefore do not show yesterday's view after the Mac has been asleep or the app has stayed in the menu bar.
