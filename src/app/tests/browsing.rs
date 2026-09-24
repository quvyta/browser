//! A page from where the person clicks, types and scrolls: the page drawn and named, links, the
//! address bar, back and forward, the wheel, fields, and Esc on a page that is loading.

use qframe::event::MouseKind;
use serde_json::json;

use super::{
    PAGE_TOP, Scratch, Slot, cell_of, click_icon, eval, find_in_row, open, open_on, page, page_drawn, until, until_page,
};

#[test]
fn the_start_address_is_drawn_and_its_tab_named_by_its_title() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open_on(scratch.machine(), Some(&links));
    until(&mut h, "the page drawn and named", |h| page_drawn(h) && find_in_row(h, "Links", 0).is_some());
    assert!(find_in_row(&h, &links, 1).is_some(), "the address bar shows the address:\n{}", h.screen());
    let viewport = eval(&h, "[innerWidth, innerHeight]");
    assert_eq!(viewport, json!([1000, 600]), "the page is laid out for the page area, 100 × 30 cells of 10 × 20");
}

#[test]
fn clicking_a_link_where_it_is_drawn_follows_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    let (x, y) = cell_of(&h, "#next");
    assert!(y >= PAGE_TOP);
    h.click(x, y);
    let second = page("/second");
    until(&mut h, "the second page", |h| h.app().address() == second && find_in_row(h, &second, 1).is_some());
    assert_eq!(eval(&h, "location.href"), json!(second), "Chromium is where the address bar says");
}

#[test]
fn an_address_typed_after_clicking_the_address_bar_is_gone_to() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    let (x, y) = find_in_row(&h, &links, 1).expect("the address on the toolbar");
    h.click(x, y);
    let third = page("/third");
    h.type_text(&third).press("enter");
    until_page(&mut h, "document.title === 'Third'");
    assert_eq!(h.app().address(), third);
    until(&mut h, "the tab named Third", |h| find_in_row(h, "Third", 0).is_some());
}

#[test]
fn back_and_forward_on_the_toolbar_walk_the_history() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let links = page("/links");
    let mut h = open(&scratch, &links);
    let (x, y) = cell_of(&h, "#next");
    h.click(x, y);
    let second = page("/second");
    until(&mut h, "the second page", |h| h.app().address() == second);
    click_icon(&mut h, "chevron-left");
    until(&mut h, "the way back", |h| h.app().address() == links);
    assert_eq!(eval(&h, "location.href"), json!(links));
    click_icon(&mut h, "chevron-right");
    until(&mut h, "the way forward", |h| h.app().address() == second);
    assert_eq!(eval(&h, "location.href"), json!(second));
}

#[test]
fn the_wheel_over_the_page_scrolls_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/long"));
    assert_eq!(eval(&h, "scrollY"), json!(0));
    for _ in 0..3 {
        h.mouse(MouseKind::ScrollDown, 20, PAGE_TOP + 5);
    }
    until_page(&mut h, "scrollY >= 180");
}

#[test]
fn keys_typed_after_clicking_a_field_of_the_page_land_in_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/form"));
    let (x, y) = cell_of(&h, "#field");
    h.click(x, y);
    until_page(&mut h, "document.activeElement.id === 'field'");
    h.type_text("Hi there");
    until_page(&mut h, "document.querySelector('#field').value === 'Hi there'");
    h.paste(", you");
    until_page(&mut h, "document.querySelector('#field').value === 'Hi there, you'");
}

#[test]
fn esc_on_a_loading_page_stops_it() {
    let _slot = Slot::take();
    let scratch = Scratch::new();
    let mut h = open(&scratch, &page("/links"));
    h.press("ctrl+l").type_text(&page("/slow")).press("enter");
    until(&mut h, "the slow page loading", |h| {
        find_in_row(h, &h.env().icons().glyph("close"), 1).is_some()
            && eval(h, "document.readyState") == json!("loading")
    });
    h.press("esc");
    until(&mut h, "the load stopped", |h| {
        find_in_row(h, &h.env().icons().glyph("browser.reload"), 1).is_some()
            && eval(h, "document.readyState !== 'loading'") == json!(true)
    });
}
