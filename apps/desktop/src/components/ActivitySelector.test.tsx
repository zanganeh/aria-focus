import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ActivitySelector } from "./ActivitySelector";

afterEach(cleanup);

it("shows five equal direct-start activity tiles with artwork", () => {
  render(<ActivitySelector disabled={false} onSelect={vi.fn().mockResolvedValue(undefined)} />);

  const tiles = screen.getAllByRole("button", { name: /^Start / });
  expect(tiles).toHaveLength(5);
  expect(screen.getByRole("button", { name: "Start Deep Work" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Start Motivation" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Start Creativity" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Start Learning" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Start Light Work" })).toBeTruthy();
  expect(document.querySelectorAll(".activity-tile-art")).toHaveLength(5);
});

it("starts the selected activity directly", () => {
  const onSelect = vi.fn().mockResolvedValue(undefined);
  render(<ActivitySelector disabled={false} onSelect={onSelect} />);

  fireEvent.click(screen.getByRole("button", { name: "Start Creativity" }));
  expect(onSelect).toHaveBeenCalledWith("creativity");
});

it("disables every direct-start tile when audio cannot be used", () => {
  render(<ActivitySelector disabled onSelect={vi.fn().mockResolvedValue(undefined)} />);

  for (const tile of screen.getAllByRole("button", { name: /^Start / })) {
    expect(tile.matches(":disabled")).toBe(true);
  }
});
