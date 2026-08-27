import { describe, expect, test } from "bun:test";
import { revealWithin } from "./reveal";

function scrollContainer(options: { height: number; headerHeight?: number }) {
  const scrolls: { top: number; left: number }[] = [];
  const header = options.headerHeight === undefined ? null : {
    getBoundingClientRect: () => ({ height: options.headerHeight }),
  };
  const element = {
    getBoundingClientRect: () => ({ top: 0, left: 0 }),
    clientHeight: options.height,
    clientWidth: 200,
    scrollBy: (delta: { top: number; left: number }) => { scrolls.push(delta); },
    querySelector: () => header,
  } as unknown as HTMLElement;
  return { element, scrolls };
}

function row(top: number, bottom: number) {
  return {
    getBoundingClientRect: () => ({ top, bottom, left: 0, right: 0 }),
  } as unknown as Element;
}

describe("revealWithin", () => {
  test("scrolls a row that a sticky header covers into full view", () => {
    const container = scrollContainer({ height: 500, headerHeight: 80 });

    revealWithin(row(40, 120), container.element, false);

    expect(container.scrolls).toEqual([{ top: -40, left: 0 }]);
  });

  test("leaves a row that already clears the sticky header alone", () => {
    const container = scrollContainer({ height: 500, headerHeight: 80 });

    revealWithin(row(90, 170), container.element, false);

    expect(container.scrolls).toEqual([]);
  });

  test("scrolls a row below the container back into view", () => {
    const container = scrollContainer({ height: 500, headerHeight: 80 });

    revealWithin(row(480, 560), container.element, false);

    expect(container.scrolls).toEqual([{ top: 60, left: 0 }]);
  });

  test("uses the whole container when it has no sticky header", () => {
    const container = scrollContainer({ height: 500 });

    revealWithin(row(-10, 70), container.element, false);

    expect(container.scrolls).toEqual([{ top: -10, left: 0 }]);
  });
});
