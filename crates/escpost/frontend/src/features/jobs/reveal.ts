/** Marks a header that stays at the top of a scroll container. */
export const STICKY_HEADER = "data-sticky-header";

/** How much of the top of `container` a sticky header covers. */
function stickyInset(container: HTMLElement): number {
  const header = container.querySelector?.(`[${STICKY_HEADER}]`);
  return header?.getBoundingClientRect().height ?? 0;
}

/** Scrolls `element` into view inside `container`, without moving the page. */
export function revealWithin(
  element: Element | undefined,
  container: HTMLElement | null,
  horizontal: boolean,
) {
  if (!element || !container) return;
  const item = element.getBoundingClientRect();
  const bounds = container.getBoundingClientRect();
  // A sticky header hides the top of the container, thus a row that reaches
  // under it is not yet visible to the reader.
  const visibleTop = bounds.top + stickyInset(container);
  const bottom = bounds.top + container.clientHeight;
  const right = bounds.left + container.clientWidth;
  let top = 0;
  let left = 0;
  if (item.top < visibleTop) top = item.top - visibleTop;
  else if (item.bottom > bottom) top = item.bottom - bottom;
  if (horizontal && item.left < bounds.left) left = item.left - bounds.left;
  else if (horizontal && item.right > right) left = item.right - right;
  if (top !== 0 || left !== 0) container.scrollBy?.({ top, left });
}
