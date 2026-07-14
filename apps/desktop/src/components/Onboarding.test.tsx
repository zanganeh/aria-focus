import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { Onboarding } from "./Onboarding";

it("is keyboard-accessible, caps genres, and starts only after its explicit action", async () => {
  const complete = vi.fn().mockResolvedValue(undefined);
  render(<Onboarding onComplete={complete} />);
  expect(screen.getByRole("group", { name: "Starting stimulation" })).toBeTruthy();
  fireEvent.click(screen.getByRole("radio", { name: /Sound-sensitive/ }));
  for (const genre of ["Atmospheric", "Lo-Fi", "Electronic"])
    fireEvent.click(screen.getByRole("checkbox", { name: genre }));
  expect(screen.getByRole("checkbox", { name: "Piano" }).hasAttribute("disabled")).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Start 30-minute Deep Work" }));
  await Promise.resolve();
  expect(complete).toHaveBeenCalledWith("low", ["atmospheric", "electronic", "lo_fi"]);
});

afterEach(cleanup);

it("keeps the form available for retry when completion fails", async () => {
  const complete = vi.fn().mockRejectedValue(new Error("audio unavailable"));
  render(<Onboarding onComplete={complete} />);
  fireEvent.click(screen.getByRole("button", { name: "Start 30-minute Deep Work" }));
  expect((await screen.findByRole("alert")).textContent).toMatch(/audio unavailable/);
  expect(
    screen.getByRole("button", { name: "Start 30-minute Deep Work" }).hasAttribute("disabled"),
  ).toBe(false);
});
