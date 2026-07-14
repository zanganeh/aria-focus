import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SessionTypeSelector } from "./SessionTypeSelector";

afterEach(cleanup);

describe("SessionTypeSelector", () => {
  it("uses native timer radios and compact defaults", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(
      <SessionTypeSelector value={{ kind: "infinite" }} disabled={false} onChange={onChange} />,
    );

    expect(screen.getByRole("radio", { name: "Infinite" })).toBeTruthy();
    await user.click(screen.getByRole("radio", { name: "Interval" }));
    expect(onChange).toHaveBeenCalledWith({
      kind: "interval",
      work_seconds: 1_500,
      break_seconds: 300,
      repeats: 4,
    });
  });

  it("applies countdown presets and validates custom bounds", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(
      <SessionTypeSelector
        value={{ kind: "countdown", seconds: 1_500 }}
        disabled={false}
        onChange={onChange}
      />,
    );
    await user.selectOptions(screen.getByLabelText("Countdown duration"), "45");
    expect(onChange).toHaveBeenCalledWith({ kind: "countdown", seconds: 2_700 });

    rerender(
      <SessionTypeSelector
        value={{ kind: "countdown", seconds: 1_200 }}
        disabled={false}
        onChange={onChange}
      />,
    );
    const custom = screen.getByLabelText("Custom countdown minutes");
    await user.clear(custom);
    await user.type(custom, "0");
    await user.click(screen.getByRole("button", { name: "Apply countdown" }));
    expect(screen.getByRole("alert").textContent).toContain("1 to 480 minutes");
  });

  it("validates interval fields and explains silent breaks", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(
      <SessionTypeSelector
        value={{ kind: "interval", work_seconds: 1_500, break_seconds: 300, repeats: 4 }}
        disabled={false}
        onChange={onChange}
      />,
    );
    expect(screen.getByText("Breaks are silent in this version.")).toBeTruthy();
    const repeats = screen.getByLabelText("Interval rounds");
    await user.clear(repeats);
    await user.type(repeats, "13");
    await user.click(screen.getByRole("button", { name: "Apply interval" }));
    expect(screen.getByRole("alert").textContent).toContain("rounds 1–12");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("disables all configuration while active", () => {
    const { container } = render(
      <SessionTypeSelector value={{ kind: "infinite" }} disabled onChange={() => undefined} />,
    );
    expect((container.querySelector("fieldset") as HTMLFieldSetElement).disabled).toBe(true);
    expect(screen.getByText("Stop the session to change its timer.")).toBeTruthy();
  });
});
