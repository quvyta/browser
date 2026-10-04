# Changelog

Every release of quvyta-browser, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## 0.2.0 - 2026-10-04

### Added

- A page's drop-down list opens on a click as qbrow's own list of its options, just under it: headless Chromium never draws it, so before the options could not be seen. Alt+↓, F4 or Space open it from the keyboard.
- Copy from the page: ctrl+c copies the page's selection, and a right click on the page opens qbrow's own menu to copy it clean or raw, go back, forward, reload or stop, and open or copy the link under the pointer.
- F1 lists every key, read from the keymap, as `?` does in every Quvyta application; on the page `?` stays the page's.
- Reading mode (F9 or the button beside the star): the page's own text in the terminal's own writing, for terminals that draw pages too small to read and for those that draw no picture at all.
- History: the small arrow beside back and forward lists the tab's own steps, and ctrl+h shows the pages this profile has seen, grouped by day, with a way to take one out.
- A page's alert, confirm and prompt boxes, and the question before leaving a page, show over its tab and the answer reaches the page; before, the page froze with nothing on screen.
- The tabs open when qbrow ends come back at the next start.
- Zoom per tab: ctrl++ and ctrl+-, ctrl+0 back to 100%, and a button on the toolbar with the level.
- A settings page (ctrl+, or the gear): the look shared by the Quvyta applications, the update notice, the search engine and the start page.

### Fixed

- qbrow talks to Chromium over a pair of pipes instead of a port on 127.0.0.1, which any program or user on the computer could connect to.
- Chromium is ended with a signal qbrow sends itself, so none is left behind where the `kill` program is missing.
- A bookmark kept in a second window is no longer written over by the first.
- After the terminal's font size changes the page is laid out again for it, so clicks land where they are aimed.
- A tab opened behind another one is drawn once it is shown, and a tab closed while Chromium was still opening it stays closed.
- ctrl+- zooms out in terminals that send it as ctrl+_.
- In a terminal drawing with half blocks Chromium sends pictures of two pixels a cell, the most such a screen shows, instead of the page's full size: far less work for qbrow and for Chromium on every frame.
- What Chromium sends reaches the screen at once instead of on the next sixtieth of a second, and qbrow no longer wakes sixty times a second while nothing happens.
- A look another Quvyta application changes while qbrow is open is followed at once.
- Chromium starts without the runtime folder, so it cannot reach the desktop's Wayland either.
- In ASCII the star is `o` and, for a kept page, `@`: a kept page's `*` was the settings button's sign, and an open star's `+` the zoom's.
- The star of a kept page is in the accent colour as well as filled; the "Chromium is missing", "Chromium stopped" and crashed-tab screens show their state in colour with its sign, and their buttons sit side by side.

## 0.1.1 - 2026-09-24

### Added

- Bookmarks: the star and ctrl+d keep a page, a bar below the address bar opens them (a middle click in a new tab, a right click to remove one), and the address bar suggests them while you type.

### Fixed

- A tab no longer stretches across the whole strip: tabs share it up to a desktop browser's width and scroll sideways once they no longer fit.

## 0.1.0 - 2026-09-24

The first version.

### Added

- A web browser inside the terminal: an unmodified Chromium runs headless, and its pages are drawn in real pixels where the terminal speaks kitty or sixel, with half blocks everywhere else.
- Tabs with a close mark and `+`, pages that open new windows opening them as tabs, back, forward, reload and stop, an address bar that also searches, and a loading mark.
- Clicks, the wheel, typing and pasting reach the page.
- A persistent profile of its own, a temporary one for a second window, and no Chromium process left behind.
- When Chromium is missing, an offer to install it in the terminal, or to show the command.
- Nine languages, and a notice when a newer version is out.
