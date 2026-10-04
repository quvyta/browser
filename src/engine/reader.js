// The page's own text, turned into Markdown where the page's own scripts cannot see it, so the
// same writing can be read in every terminal whatever the page's own layout is. Nothing here
// writes, inserts or removes anything: the page is left exactly as it was found.
(() => {
    'use strict';

    // Markdown's own signs, escaped wherever the page wrote them, so a page full of asterisks and
    // hashes reads as words rather than as emphasis and headings.
    const SIGNS = /([\\`*_\[\]#|])/g;

    // What is the page's furniture rather than its text: an element that is one of these, or is
    // inside one, says nothing about what the page has to say.
    const FURNITURE =
        'nav,header,footer,aside,form,script,style,noscript,template,button,input,select,textarea,iframe';

    // The blocks a reading is made of; anything else among a block's children is part of its words.
    const BLOCKS = new Set([
        'ADDRESS', 'ARTICLE', 'ASIDE', 'BLOCKQUOTE', 'BODY', 'DD', 'DIV', 'DL', 'DT', 'FIELDSET',
        'FIGURE', 'FOOTER', 'FORM', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'HEADER', 'HTML', 'LI',
        'MAIN', 'NAV', 'OL', 'P', 'PRE', 'SECTION', 'TABLE', 'UL',
    ]);

    // A picture with words of its own is told on a line of its own, marked so that the application
    // can name it in the language on screen; where the picture came from is of no interest here.
    const MARKER = '[image: ';
    const MARKED = /\[image: ([^\]]*)\]/g;

    const escape = (text) => text.replace(SIGNS, '\\$1');

    // The page's words as Markdown, whitespace made one space and the signs escaped.
    const words = (text) => escape(text.replace(/\s+/g, ' ').trim());

    // Whether this element is the page's furniture or something the page has hidden away.
    const furniture = (element) => {
        if (element.matches(FURNITURE)) return true;
        if (element.hasAttribute('hidden') || element.getAttribute('aria-hidden') === 'true') return true;
        const style = window.getComputedStyle(element);
        return style.display === 'none' || style.visibility === 'hidden';
    };

    // Markdown needs a blank line between two blocks: a row of dashes straight after a paragraph
    // is read as the second line of a heading rather than as the rule it is.
    const gap = (out) => {
        if (out.length) out.push('');
    };

    // A line beginning with Markdown's own sign of a list or a quote would be read as one, though
    // the page wrote a word there.
    const begun = (line) => line.replace(/^([-+\>|])/, '\\$1');

    // One line of text, with a picture's line taken out to stand on its own.
    const lines = (text, out) => {
        MARKED.lastIndex = 0;
        let last = 0;
        let found = MARKED.exec(text);
        if (!found) {
            out.push(begun(text));
            return;
        }
        while (found) {
            const before = text.slice(last, found.index).trim();
            if (before) out.push(begun(before));
            out.push(found[0]);
            last = found.index + found[0].length;
            found = MARKED.exec(text);
        }
        const after = text.slice(last).trim();
        if (after) out.push(begun(after));
    };

    // What a picture says of itself, and nothing else: an address in the reading would be noise.
    const picture = (element) => {
        const alt = escape((element.getAttribute('alt') || '').replace(/\s+/g, ' ').replace(/[\[\]]/g, '').trim());
        return alt ? MARKER + alt + ']' : '';
    };

    // The text inside a block: emphasis, inline code, and the words of a link without its address.
    const inline = (node) => {
        let out = '';
        for (const child of node.childNodes) {
            if (child.nodeType === 3) {
                out += escape(child.nodeValue || '');
                continue;
            }
            if (child.nodeType !== 1 || furniture(child)) continue;
            const tag = child.tagName;
            if (tag === 'BR') {
                out += ' ';
                continue;
            }
            if (tag === 'IMG') {
                out += picture(child);
                continue;
            }
            if (tag === 'CODE' || tag === 'KBD' || tag === 'SAMP' || tag === 'VAR') {
                const text = words(child.textContent || '');
                if (text) out += '`' + text.replace(/`/g, '\\`') + '`';
                continue;
            }
            const inner = inline(child).trim();
            if (!inner) continue;
            if (tag === 'STRONG' || tag === 'B') out += '**' + inner + '**';
            else if (tag === 'EM' || tag === 'I') out += '*' + inner + '*';
            else out += inner;
        }
        return out;
    };

    // The blocks of `element` one after another, lists nested as deep as the page nests them, and
    // whether this is the inside of a list item, where a nested list is not a block of its own.
    const blocks = (element, out, depth, item) => {
        let pending = '';
        const flush = () => {
            const text = pending.replace(/\s+/g, ' ').trim();
            pending = '';
            if (text) {
                gap(out);
                lines(text, out);
            }
        };
        for (const node of element.childNodes) {
            if (node.nodeType === 3) {
                pending += escape(node.nodeValue || '');
            } else if (node.nodeType === 1 && !furniture(node)) {
                if (BLOCKS.has(node.tagName)) {
                    flush();
                    block(node, out, depth, item);
                } else if (node.tagName === 'IMG') {
                    pending += picture(node);
                } else {
                    pending += inline(node);
                }
            }
        }
        flush();
    };

    // One block of the page, in Markdown.
    const block = (element, out, depth, item) => {
        const tag = element.tagName;
        if (/^H[1-6]$/.test(tag)) {
            const text = inline(element).replace(/\s+/g, ' ').trim();
            if (text) {
                gap(out);
                out.push('#'.repeat(Number(tag[1])) + ' ' + text);
            }
            return;
        }
        if (tag === 'HR') {
            gap(out);
            out.push('---');
            return;
        }
        if (tag === 'PRE') {
            code(element, out);
            return;
        }
        if (tag === 'BLOCKQUOTE') {
            const inner = [];
            blocks(element, inner, depth, false);
            if (!inner.length) return;
            gap(out);
            for (const line of inner) out.push('> ' + line);
            return;
        }
        if (tag === 'UL' || tag === 'OL') {
            list(element, out, depth, item);
            return;
        }
        if (tag === 'TABLE') {
            table(element, out);
            return;
        }
        blocks(element, out, depth, false);
    };

    // A code block as it was written: its lines and its indentation are part of what it says.
    const code = (element, out) => {
        const text = (element.textContent || '').replace(/\r\n?/g, '\n').replace(/\n+$/, '');
        if (!text.trim()) return;
        gap(out);
        out.push('```');
        for (const line of text.split('\n')) out.push(line);
        out.push('```');
    };

    // A list with its depth: an item's own words, then whatever it holds nested inside it. A
    // nested list follows its item's words without a blank line, or it would be a list of its own.
    const list = (element, out, depth, item) => {
        const ordered = element.tagName === 'OL';
        let number = Number(element.getAttribute('start') || '1');
        if (!Number.isFinite(number)) number = 1;
        const items = [...element.children].filter((child) => child.tagName === 'LI' && !furniture(child));
        if (!items.length) return;
        if (!item) gap(out);
        for (const item of items) {
            const marker = ordered ? number + '. ' : '- ';
            number += 1;
            const inner = [];
            blocks(item, inner, depth, true);
            const pad = '  '.repeat(depth);
            const under = pad + ' '.repeat(marker.length);
            let first = true;
            for (const line of inner) {
                out.push((first ? pad + marker : under) + line);
                first = false;
            }
        }
    };

    // A table one row to a line, its cells told apart by the mark between them.
    const table = (element, out) => {
        let opened = false;
        for (const row of element.querySelectorAll('tr')) {
            if (furniture(row)) continue;
            const cells = [...row.children]
                .filter((cell) => cell.tagName === 'TD' || cell.tagName === 'TH')
                .map((cell) => words(cell.textContent || ''))
                .filter((cell) => cell !== '');
            if (!cells.length) continue;
            if (!opened) {
                gap(out);
                opened = true;
            }
            out.push(cells.join(' · '));
        }
    };

    // The part of the page with the most to say: its article, its main, its role, or else the
    // element whose own paragraphs hold the most words.
    const richest = (body) => {
        let best = body;
        let most = 0;
        for (const candidate of [body, ...body.querySelectorAll('*')]) {
            let said = 0;
            for (const child of candidate.children) {
                if (child.tagName === 'P') said += (child.textContent || '').trim().length;
            }
            // Only an element that could win is asked whether it is furniture: reading every
            // element's style costs the page more than counting its words does.
            if (said > most && !furniture(candidate)) {
                most = said;
                best = candidate;
            }
        }
        return best;
    };

    const body = document.body || document.documentElement;
    if (!body) return { title: document.title, text: '' };
    let root = null;
    for (const wanted of ['article', 'main', '[role=main]']) {
        root = [...document.querySelectorAll(wanted)].find((found) => !furniture(found)) || null;
        if (root) break;
    }
    const out = [];
    blocks(root || richest(body), out, 0, false);
    return { title: document.title, text: out.join('\n').replace(/\n{3,}/g, '\n\n').trim() };
})();