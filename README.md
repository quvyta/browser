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

## Keys

| | Keys | Mouse |
|---|---|---|
| Go to an address | ctrl+l or F6, type, enter | click the address |
| Back, forward | alt+←, alt+→ | the arrows |
| Reload, stop | ctrl+r or F5; Esc stops a page that is loading | the reload button, a stop button while loading |
| New tab | ctrl+t | `+` |
| Close the tab | ctrl+w | the tab's `×` |
| Next, previous tab | ctrl+PgDn, ctrl+PgUp | click the tab |
| Quit | ctrl+q | |

While the page has the keyboard (after a click on it), every other key goes to the page: Tab moves between its fields, the arrows and PgUp/PgDn scroll it, and pasting pastes into it. Closing the last tab opens an empty one; qbrow ends only with ctrl+q.

## Profile

Cookies and logins are kept in `~/.local/state/quvyta/browser/profile`, apart from any Chromium profile of your own. A second qbrow started while the first runs uses a temporary profile, says so on its toolbar and removes it when it quits. Every Chromium process a qbrow starts ends with it.

## Network

qbrow is a web browser: it reaches every site you open, and Chromium makes its own usual requests besides. Apart from that, once a day at start qbrow asks crates.io whether a newer version of `quvyta-browser` is out and says so in a notice when there is one; only the package's name and version are sent. The question never holds up the start and is silent without a network. The switch that turns it off is shared by every Quvyta application and is on their settings pages.

## Licence

MIT
