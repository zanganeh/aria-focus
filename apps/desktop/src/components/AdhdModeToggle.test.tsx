import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AdhdModeToggle } from "./AdhdModeToggle";

afterEach(cleanup);

it("keeps the ADHD state in the control indicator rather than repeating it as text", () => {
  render(<AdhdModeToggle value="medium" disabled={false} onChange={vi.fn()} />);

  expect(screen.getByRole("button", { name: "ADHD mode" })).toBeTruthy();
  expect(screen.queryByText("High stimulation")).toBeNull();
  expect(screen.queryByText("Off")).toBeNull();
});

it("toggles high stimulation from the compact control", () => {
  const onChange = vi.fn();
  render(<AdhdModeToggle value="medium" disabled={false} onChange={onChange} />);

  fireEvent.click(screen.getByRole("button", { name: "ADHD mode" }));
  expect(onChange).toHaveBeenCalledWith("high");
});
