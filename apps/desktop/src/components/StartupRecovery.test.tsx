import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { StartupRecovery } from "./StartupRecovery";

it("does not render when every startup service is ready", () => {
  render(
    <StartupRecovery
      health={{ core_ready: true, core_error: null, packs_ready: true, packs_error: null }}
      busy={false}
      retryError={null}
      onRetry={vi.fn()}
    />,
  );
  expect(screen.queryByRole("button", { name: "Retry startup" })).toBeNull();
});

it("shows a migration conflict without hiding otherwise healthy services", () => {
  const onRetry = vi.fn();
  render(
    <StartupRecovery
      health={{
        core_ready: true,
        core_error: null,
        packs_ready: true,
        packs_error: null,
        migration_status: "conflict",
        migration_error: "Both the old and new data folders contain files.",
      }}
      busy={false}
      retryError={null}
      onRetry={onRetry}
    />,
  );

  expect(screen.getByText(/could not be migrated safely/i)).toBeTruthy();
  expect(screen.getByText(/both the old and new data folders/i)).toBeTruthy();
  expect(screen.queryByText(/session and audio controls are unavailable/i)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Retry startup" }));
  expect(onRetry).toHaveBeenCalledOnce();
});
