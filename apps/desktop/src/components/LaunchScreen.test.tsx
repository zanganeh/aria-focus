import { render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { cleanup } from "@testing-library/react";
import { LaunchScreen } from "./LaunchScreen";

afterEach(cleanup);

it("shows the restrained brand while startup work is pending", () => {
  render(<LaunchScreen label="Loading local preferences" />);
  expect(screen.getByRole("main", { name: "Loading local preferences" })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Aria Focus" })).toBeTruthy();
  expect(screen.queryByText(/Aria Zanganeh/)).toBeNull();
});
