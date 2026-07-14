import { useState } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Intensity } from "../lib/types";
import { IntensitySelector } from "./IntensitySelector";

afterEach(cleanup);

function StatefulSelector({ onChange }: { onChange: (value: Intensity) => void }) {
  const [value, setValue] = useState<Intensity>("low");

  return (
    <IntensitySelector
      value={value}
      disabled={false}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

describe("IntensitySelector", () => {
  it("exposes all intensity choices as native radio controls with checked state", () => {
    render(<IntensitySelector value="medium" disabled={false} onChange={() => undefined} />);

    const radios = screen.getAllByRole("radio") as HTMLInputElement[];
    expect(radios).toHaveLength(4);
    expect(radios.map((radio) => radio.type)).toEqual(["radio", "radio", "radio", "radio"]);
    expect((screen.getByRole("radio", { name: /^Off\./ }) as HTMLInputElement).checked).toBe(false);
    expect((screen.getByRole("radio", { name: /^Medium\./ }) as HTMLInputElement).checked).toBe(
      true,
    );
    expect(
      (
        screen.getByRole("radio", {
          name: /^High \/ ADHD\./,
        }) as HTMLInputElement
      ).checked,
    ).toBe(false);
  });

  it("supports pointer selection and native arrow-key navigation", async () => {
    Object.defineProperty(window, "CSS", {
      configurable: true,
      value: { escape: (value: string) => value },
    });
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<StatefulSelector onChange={onChange} />);

    const medium = screen.getByRole("radio", { name: /^Medium\./ });
    const high = screen.getByRole("radio", { name: /^High \/ ADHD\./ });

    await user.click(high);
    expect((high as HTMLInputElement).checked).toBe(true);
    expect(onChange).toHaveBeenLastCalledWith("high");

    await user.keyboard("{ArrowLeft}");
    expect((medium as HTMLInputElement).checked).toBe(true);
    expect(onChange).toHaveBeenLastCalledWith("medium");
  });
});
