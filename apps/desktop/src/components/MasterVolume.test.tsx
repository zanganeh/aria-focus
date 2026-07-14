import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { MasterVolume } from "./MasterVolume";

vi.mock("./AppIcon", () => ({
  AppIcon: ({ name }: { name: string }) => <svg data-icon={name} aria-hidden="true" />,
}));

afterEach(() => {
  cleanup();
});

it("uses an accessible native range and reports saving state", async () => {
  const onChange = vi.fn();
  render(<MasterVolume value={70} pending={true} disabled={false} onChange={onChange} />);
  const slider = screen.getByRole("slider", { name: /Master volume/ }) as HTMLInputElement;
  expect(slider.min).toBe("0");
  expect(slider.max).toBe("100");
  expect(slider.value).toBe("70");
  expect(screen.getByText("Saving volume…")).toBeTruthy();
  fireEvent.change(slider, { target: { value: "69" } });
  expect(onChange).toHaveBeenCalledWith(69);
});

it("compact variant renders the current percentage and a quiet live region", () => {
  const { container } = render(
    <MasterVolume
      variant="compact"
      value={70}
      pending={false}
      disabled={false}
      onChange={vi.fn()}
    />,
  );
  expect(screen.getByText("70%")).toBeTruthy();
  expect(container.querySelector('[data-icon="speaker"]')).toBeTruthy();
  expect(container.querySelector('[data-icon="speaker-muted"]')).toBeNull();
  const status = screen.getByText("Volume is saved on this device.");
  expect(status.getAttribute("aria-live")).toBe("polite");
  expect(status.closest(".visually-hidden")).toBeTruthy();
});

it("compact variant uses the muted speaker icon at 0", () => {
  const { container } = render(
    <MasterVolume
      variant="compact"
      value={0}
      pending={false}
      disabled={false}
      onChange={vi.fn()}
    />,
  );
  expect(container.querySelector('[data-icon="speaker-muted"]')).toBeTruthy();
  expect(container.querySelector('[data-icon="speaker"]')).toBeNull();
});

it("compact variant calls onChange and disables correctly", () => {
  const onChange = vi.fn();
  render(
    <MasterVolume
      variant="compact"
      value={42}
      pending={false}
      disabled={true}
      onChange={onChange}
    />,
  );
  const slider = screen.getByRole("slider", { name: /Master volume/ }) as HTMLInputElement;
  expect(slider.value).toBe("42");
  expect(slider.disabled).toBe(true);
  fireEvent.change(slider, { target: { value: "43" } });
  expect(onChange).toHaveBeenCalledWith(43);
});

it("compact variant exposes pending state accessibly", () => {
  const { container } = render(
    <MasterVolume
      variant="compact"
      value={70}
      pending={true}
      disabled={false}
      onChange={vi.fn()}
    />,
  );
  const region = screen.getByText("Saving volume…");
  expect(region.getAttribute("aria-live")).toBe("polite");
  expect(region.closest(".visually-hidden")).toBeTruthy();
  expect(container.querySelector(".player-volume")?.getAttribute("aria-busy")).toBe("true");
});
