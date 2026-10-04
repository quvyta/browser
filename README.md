# qbrow

A real web browser inside the terminal. An unmodified Chromium runs out of sight and draws the pages; qbrow drives it from outside through the DevTools protocol, shows what it draws and hands it your clicks, the wheel and your keys. The tabs, the address bar and the buttons are the terminal's own.

Because Chromium itself is never changed, a new Chromium release does not break qbrow: the pages look and behave as they do in Chromium, logins and all.

## Using it

```
qbrow [ADDRESS] [--profile FOLDER]
```

`quvyta-browser` is the same program under its long name. An address without a scheme gets `https://`, a local name or IP address gets `http://`, and anything that is not an address is searched for with DuckDuckGo. A file or folder that exists opens as a `file://` address.

qbrow needs Chromium (or Google Chrome). It looks for `QBROW_CHROMIUM`, then `chromium`, `chromium-browser`, `google-chrome-stable` and `google-chrome` on the search path. When none is there it says so, and offers to install Chromium with your package manager (pacman, apt, dnf or zypper) in the terminal in front of you, or just to show the command; nothing is installed until you ask.

## Pictures

Where the terminal speaks the kitty graphics protocol or sixel (kitty, WezTerm, Ghostty, Konsole, foot and others), the page is drawn in real pixels. Everywhere else, over SSH and inside tmux too, every cell shows two pixels with half blocks: the page's layout and pictures are there, small text is not readable. `QUVYTA_GRAPHICS=halfblock` chooses half blocks anywhere.

Reading mode (F9, or the button beside the star) puts the page's own text where its picture is, written with the terminal's own letters: headings, paragraphs, lists, quotes, code and tables, without the menus, sidebars and footers around them. It reads at any font size and on a terminal that draws no picture at all, where the empty page has a button for it. F9, the button or Esc puts the page back; a tab keeps its reading until it goes to another address. Links are not followed from the reading.

## Keys

| | Keys | Mouse |
|---|---|---|
| Go to an address | ctrl+l or F6, type, enter | click the address |
| Back, forward | alt+←, alt+→ | the arrows |
| Reload, stop | ctrl+r or F5; Esc stops a page that is loading | the reload button, a stop button while loading |
| New tab | ctrl+t | `+` |
| Close the tab | ctrl+w | the tab's `×` |
| Next, previous tab | ctrl+PgDn, ctrl+PgUp | click the tab |
| Bookmark the page, or remove it | ctrl+d | the star |
| Open a bookmark | type part of its name or address, ↓ ↑, enter | click it on the bar; middle click opens it in a new tab |
| Open a page's drop-down list | Tab to it, then alt+↓, F4 or space; ↑ ↓ or a letter, enter | click it, then click an option |
| Copy what is selected on the page | ctrl+c | right click → Copy, or Copy raw to keep its lines |
| The pages seen before | ctrl+h | the small arrow beside back or forward lists the tab's own steps |
| Zoom in, out, back to 100% | ctrl++ (or ctrl+=), ctrl+-, ctrl+0 | the zoom button's list |
| Reading mode, and back to the page | F9; Esc closes it | the button beside the star |
| Settings | ctrl+, | the gear |
| Every key, listed | F1; `?` where the page does not have the keyboard | |
| Quit | ctrl+q | |

While the page has the keyboard (after a click on it), every other key goes to the page: Tab moves between its fields, the arrows and PgUp/PgDn scroll it, and pasting pastes into it. Closing the last tab opens an empty one; qbrow ends only with ctrl+q, and the tabs open then come back at the next start. A page's own alert, confirm and prompt boxes show over its tab and wait for your answer; Enter is OK and Esc is Cancel.

## Bookmarks

The star at the end of the address bar keeps the page as a bookmark, and a second press lets it go; ctrl+d does the same. Once there is a bookmark, a bar below the address bar shows them in the order they were added, and the ones that do not fit are under More. A click opens a bookmark in the tab, a middle click in a new tab, and a right click offers both and Remove. Typing in the address bar lists the bookmarks whose name or address holds what you typed; ↓ and ↑ choose one, enter or a click opens it, Esc closes the list and keeps your text, and enter without a choice goes where you typed.

The bookmarks are kept in `~/.local/share/quvyta/browser/bookmarks`, one per line: the address, a tab, and the name.

## History

The small arrow beside back and beside forward lists the tab's own steps to that side; choosing one goes there, a middle click opens it in a new tab. ctrl+h shows the pages this profile has seen, newest first and grouped by day: enter opens one, a middle click opens it in a new tab, and Delete takes it out after asking. A page seen again is one entry at its newest time, and the newest 5000 are kept. The list is kept beside the browser profile in `~/.local/state/quvyta/browser/history`, never sent anywhere; a window that runs on a temporary profile keeps its own and takes it away when it closes.

## Profile

Cookies and logins are kept in `~/.local/state/quvyta/browser/profile`, apart from any Chromium profile of your own. A second qbrow started while the first runs uses a temporary profile, says so on its toolbar and removes it when it quits. Every Chromium process a qbrow starts ends with it.

## Network

qbrow is a web browser: it reaches every site you open, and Chromium makes its own usual requests besides. Apart from that, once a day at start qbrow asks crates.io whether a newer version of `quvyta-browser` is out and says so in a notice when there is one; only the package's name and version are sent. The question never holds up the start and is silent without a network. The switch that turns it off is shared by every Quvyta application and is on their settings pages, qbrow's included (ctrl+, or the gear). qbrow talks to its Chromium over a pair of pipes, not a network port, so no other program or user on the computer can connect to it.

## Licence

MIT
