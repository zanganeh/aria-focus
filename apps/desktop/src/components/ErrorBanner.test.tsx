import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ErrorBanner } from "./ErrorBanner";

afterEach(cleanup);

describe("ErrorBanner", () => {
  it("announces a native command error and can be dismissed", async () => {
    const onDismiss = vi.fn();
    const user = userEvent.setup();
    render(<ErrorBanner message="The output device is unavailable." onDismiss={onDismiss} />);

    expect(screen.getByRole("alert").textContent).toContain("The output device is unavailable.");
    await user.click(screen.getByRole("button", { name: "Dismiss error message" }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("renders nothing when there is no error", () => {
    render(<ErrorBanner message={null} onDismiss={() => undefined} />);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
