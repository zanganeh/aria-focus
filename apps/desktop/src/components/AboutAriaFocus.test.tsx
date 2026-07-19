import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AboutAriaFocus } from "./AboutAriaFocus";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

afterEach(cleanup);

it("keeps personal attribution in About without making medical claims", () => {
  render(<AboutAriaFocus />);
  expect(screen.getByRole("heading", { name: "Aria Focus" })).toBeTruthy();
  expect(screen.getByText("Aria Zanganeh")).toBeTruthy();
  expect(screen.getByText("github.com/zanganeh/aria-focus")).toBeTruthy();
  expect(screen.getByText(/not medical treatment/i)).toBeTruthy();
});

it("provides external GitHub help and feedback links", () => {
  vi.mocked(openUrl).mockResolvedValue();
  render(<AboutAriaFocus />);
  expect(screen.getByRole("heading", { name: "Help & feedback" })).toBeTruthy();
  expect(screen.getByRole("link", { name: "Report a bug" }).getAttribute("target")).toBe("_blank");
  expect(screen.getByRole("link", { name: "Request a feature" }).getAttribute("href")).toContain(
    "feature_request.yml",
  );
  expect(screen.getByRole("link", { name: "Open issues" }).getAttribute("href")).toContain(
    "/issues",
  );
  expect(screen.getByRole("link", { name: "Source & releases" }).getAttribute("href")).toContain(
    "/releases",
  );
  fireEvent.click(screen.getByRole("link", { name: "Report a bug" }));
  expect(openUrl).toHaveBeenCalledWith(
    "https://github.com/zanganeh/aria-focus/issues/new?template=bug_report.yml",
  );
});
