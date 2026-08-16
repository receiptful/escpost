import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, render, screen } from "@testing-library/preact";
import { App } from "./app";

afterEach(cleanup);

describe("App", () => {
  test("identifies the new workbench without advertising unfinished controls", () => {
    render(<App />);

    expect(
      screen.getByRole("heading", { name: "ESCPost workbench" }),
    ).toBeTruthy();
    expect(
      screen.getByText("The new web workbench is under construction."),
    ).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
