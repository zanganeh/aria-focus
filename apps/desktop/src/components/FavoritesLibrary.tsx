import { useEffect, useMemo, useState } from "react";
import { listFavorites, removeFavorite, startFavorite } from "../lib/api";
import type { FavoriteLibraryItem } from "../lib/types";

export function FavoritesLibrary({
  active,
  disabled,
  revision,
  onStarted,
  onError,
}: {
  active: boolean;
  disabled: boolean;
  revision: number;
  onStarted: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const [items, setItems] = useState<FavoriteLibraryItem[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const pageSize = 3;

  useEffect(() => {
    let current = true;
    void listFavorites()
      .then((next) => current && setItems(next))
      .catch((error: unknown) => {
        if (current)
          onError(
            `Unable to load Favorites: ${error instanceof Error ? error.message : String(error)}`,
          );
      });
    return () => {
      current = false;
    };
  }, [onError, revision]);

  const remove = (item: FavoriteLibraryItem) => {
    setBusy(`remove:${item.item_id}:${item.activity}`);
    void removeFavorite(item.item_id, item.activity)
      .then(() =>
        setItems(
          (current) =>
            current?.filter(
              (entry) => entry.item_id !== item.item_id || entry.activity !== item.activity,
            ) ?? current,
        ),
      )
      .catch((error: unknown) =>
        onError(
          `Unable to remove favorite: ${error instanceof Error ? error.message : String(error)}`,
        ),
      )
      .finally(() => setBusy(null));
  };
  const start = (item: FavoriteLibraryItem) => {
    setBusy(`start:${item.item_id}:${item.activity}`);
    void startFavorite(item.item_id, item.activity)
      .then(onStarted)
      .catch((error: unknown) =>
        onError(
          `Unable to start favorite: ${error instanceof Error ? error.message : String(error)}`,
        ),
      )
      .finally(() => setBusy(null));
  };

  const pageCount = Math.max(1, Math.ceil((items?.length ?? 0) / pageSize));
  const visibleItems = useMemo(
    () => (items ?? []).slice(page * pageSize, (page + 1) * pageSize),
    [items, page],
  );
  useEffect(() => setPage((current) => Math.min(current, pageCount - 1)), [pageCount]);

  return (
    <details className="favorites-library">
      <summary>Favorites library</summary>
      {items?.length === 0 && (
        <p>You have no liked tracks yet. Choose “Liked” after a track plays to save it here.</p>
      )}
      {items && items.length > 0 && (
        <ul>
          {visibleItems.map((item) => (
            <li key={`${item.activity}:${item.item_id}`}>
              <strong>{item.title}</strong> · {item.activity.replace("_", " ")} ·{" "}
              {item.genre.join(", ") || "No genre"} · {item.moods.join(", ") || "No mood"}
              <div>
                <button
                  type="button"
                  disabled={disabled || active || busy !== null}
                  onClick={() => start(item)}
                >
                  Play
                </button>
                <button type="button" disabled={busy !== null} onClick={() => remove(item)}>
                  Remove favorite
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {pageCount > 1 && (
        <div className="library-pagination" aria-label="Favourite pages">
          <button
            type="button"
            disabled={page === 0}
            onClick={() => setPage((current) => Math.max(0, current - 1))}
          >
            Previous
          </button>
          <span>
            Page {page + 1} of {pageCount}
          </span>
          <button
            type="button"
            disabled={page + 1 >= pageCount}
            onClick={() => setPage((current) => Math.min(pageCount - 1, current + 1))}
          >
            Next
          </button>
        </div>
      )}
    </details>
  );
}
