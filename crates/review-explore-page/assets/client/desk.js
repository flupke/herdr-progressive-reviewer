// The windows where the page reads in two columns, its reading column and the panel beside it
// (layout.css, from 70rem): there the chat stands at the window's left, under its bubble at the
// left of the masthead (left-column.css, masthead.css); below, the chat is a sheet and its bubble
// sits beside the masthead's menu.

export const DESK = matchMedia('(min-width: 70rem)');

/** The windows with room for a third column, at the window's left, beside the reading column and
 * the panel (layout.css, from 89rem): the chat or the list of past rounds stands there beside the
 * page, which makes room for it; below, it lies over the page. */
export const BESIDE = matchMedia('(min-width: 89rem)');
