import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GenreSelector } from "./GenreSelector";

afterEach(cleanup);

describe("GenreSelector", () => {
  it("offers Any and stable available choices, forwarding the selected ID", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(
      <GenreSelector
        disabled={false}
        onChange={onChange}
        state={{
          selected_genre_id: null,
          selected_genre_available: true,
          available_genres: [
            { id: "ambient", label: "Ambient" },
            { id: "classical", label: "Classical" },
          ],
        }}
      />,
    );
    expect(screen.getByRole("radio", { name: "Any compatible genre" })).toBeTruthy();
    await user.click(screen.getByRole("radio", { name: "Classical" }));
    expect(onChange).toHaveBeenCalledWith("classical");
  });

  it("makes an unavailable saved choice explicit and leaves recovery choices available", () => {
    render(
      <GenreSelector
        disabled={false}
        onChange={() => undefined}
        state={{
          selected_genre_id: "removed",
          selected_genre_available: false,
          available_genres: [{ id: "ambient", label: "Ambient" }],
        }}
      />,
    );
    expect(screen.getByRole("status").textContent).toMatch(/removed.*unavailable/i);
    expect(screen.getByRole("radio", { name: "Any compatible genre" })).toBeTruthy();
    expect(screen.getByRole("radio", { name: "Ambient" })).toBeTruthy();
  });

  it("disables changes while transport is active", () => {
    render(
      <GenreSelector
        disabled
        onChange={() => undefined}
        state={{
          selected_genre_id: null,
          selected_genre_available: true,
          available_genres: [{ id: "ambient", label: "Ambient" }],
        }}
      />,
    );
    for (const radio of screen.getAllByRole("radio") as HTMLInputElement[]) {
      expect(radio.matches(":disabled")).toBe(true);
    }
  });
});
