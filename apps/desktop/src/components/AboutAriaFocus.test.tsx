import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { AboutAriaFocus } from "./AboutAriaFocus";

afterEach(cleanup);

it("keeps personal attribution in About without making medical claims", () => {
  render(<AboutAriaFocus />);
  expect(screen.getByRole("heading", { name: "Aria Focus" })).toBeTruthy();
  expect(screen.getByText("Aria Zanganeh")).toBeTruthy();
  expect(screen.getByText("github.com/zanganeh/aria-focus")).toBeTruthy();
  expect(screen.getByText(/not medical treatment/i)).toBeTruthy();
});
