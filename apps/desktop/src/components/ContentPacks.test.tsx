import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { chooseAndImportContentPack, listContentPacks } from "../lib/api";
import { ContentPacks } from "./ContentPacks";

vi.mock("../lib/api", () => ({
  chooseAndImportContentPack: vi.fn(),
  listContentPacks: vi.fn(),
}));

const PACK = {
  id: "fixture.pack",
  title: "Fixture Pack",
  version: "1.2.3",
  item_count: 2,
  status: "validated_metadata" as const,
};

beforeEach(() => {
  vi.mocked(listContentPacks).mockReset();
  vi.mocked(listContentPacks).mockResolvedValue([]);
  vi.mocked(chooseAndImportContentPack).mockReset();
});

afterEach(cleanup);

describe("ContentPacks", () => {
  it("lists only safe pack metadata and explains playback eligibility", async () => {
    vi.mocked(listContentPacks).mockResolvedValue([PACK]);
    render(<ContentPacks />);

    expect(screen.getByText(/may still have no eligible playable item/i)).toBeTruthy();
    expect(await screen.findByText("Fixture Pack")).toBeTruthy();
    expect(screen.getByText("v1.2.3 · 2 items")).toBeTruthy();
    expect(document.body.textContent).not.toContain("C:\\");
  });

  it("imports through the chooser and announces success", async () => {
    vi.mocked(chooseAndImportContentPack).mockResolvedValue(PACK);
    const onCatalogueChange = vi.fn();
    const user = userEvent.setup();
    render(<ContentPacks onCatalogueChange={onCatalogueChange} />);
    await waitFor(() => expect(listContentPacks).toHaveBeenCalledOnce());

    await user.click(screen.getByRole("button", { name: "Import pack" }));
    expect((await screen.findByRole("status")).textContent).toContain(
      "Fixture Pack was imported and validated.",
    );
    expect(screen.getByText("Fixture Pack")).toBeTruthy();
    expect(onCatalogueChange).toHaveBeenCalledOnce();
  });

  it("labels a build-bundled owner-waived private-beta pack", async () => {
    vi.mocked(listContentPacks).mockResolvedValue([
      { ...PACK, status: "owner_waived_bundled_private_beta" },
    ]);
    render(<ContentPacks />);
    expect(await screen.findByText(/Private beta \/ owner waived/i)).toBeTruthy();
  });

  it("does not report a catalogue change when importing is cancelled or fails", async () => {
    const user = userEvent.setup();
    const onCatalogueChange = vi.fn();
    vi.mocked(chooseAndImportContentPack).mockResolvedValueOnce(null);
    render(<ContentPacks onCatalogueChange={onCatalogueChange} />);
    await waitFor(() => expect(listContentPacks).toHaveBeenCalledOnce());
    await user.click(screen.getByRole("button", { name: "Import pack" }));
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(onCatalogueChange).not.toHaveBeenCalled();

    vi.mocked(chooseAndImportContentPack).mockRejectedValueOnce(new Error("invalid pack"));
    await user.click(screen.getByRole("button", { name: "Import pack" }));
    expect((await screen.findByRole("alert")).textContent).toContain("invalid pack");
    expect(onCatalogueChange).not.toHaveBeenCalled();
  });

  it("does not invoke the import dialog or API while disabled", async () => {
    render(<ContentPacks disabled />);
    await waitFor(() => expect(listContentPacks).toHaveBeenCalledOnce());

    const button = screen.getByRole("button", { name: "Import pack" });
    expect(button.hasAttribute("disabled")).toBe(true);
    await userEvent.setup().click(button);
    expect(chooseAndImportContentPack).not.toHaveBeenCalled();
  });
});
