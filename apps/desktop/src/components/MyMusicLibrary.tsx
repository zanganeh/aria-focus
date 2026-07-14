import { useEffect, useState } from "react";
import { deleteMyMusic, listMyMusic, renameMyMusic, startMyMusic } from "../lib/api";
import type { MyMusicItem } from "../lib/types";

export function MyMusicLibrary({
  disabled,
  onError,
  onStarted,
  onCatalogueChange,
}: {
  disabled: boolean;
  onError: (message: string) => void;
  onStarted: () => Promise<void>;
  onCatalogueChange?: () => void;
}) {
  const [items, setItems] = useState<MyMusicItem[]>([]);
  useEffect(() => {
    void listMyMusic()
      .then(setItems)
      .catch(() => onError("My Music could not be loaded right now."));
  }, [onError]);
  if (items.length === 0) return null;
  return (
    <section aria-label="My Music">
      <h2>My Music</h2>
      {items.map((item) => (
        <article key={item.item_id}>
          <strong>{item.title}</strong>
          <p>Created on this device · {item.duration_seconds} sec</p>
          <button
            disabled={disabled}
            type="button"
            onClick={() =>
              void startMyMusic(item.item_id, item.activity)
                .then(onStarted)
                .catch(() => onError("This music could not be played right now."))
            }
          >
            Play
          </button>
          <button
            disabled={disabled}
            type="button"
            onClick={() => {
              const title = window.prompt("Rename music", item.title);
              if (title) {
                void renameMyMusic(item.item_id, title)
                  .then(() =>
                    setItems((all) =>
                      all.map((value) =>
                        value.item_id === item.item_id ? { ...value, title: title.trim() } : value,
                      ),
                    ),
                  )
                  .catch(() => onError("That name could not be saved."));
              }
            }}
          >
            Rename
          </button>
          <button
            disabled={disabled}
            type="button"
            onClick={() => {
              if (window.confirm(`Delete ${item.title}?`)) {
                void deleteMyMusic(item.item_id)
                  .then(() => {
                    setItems((all) => all.filter((value) => value.item_id !== item.item_id));
                    onCatalogueChange?.();
                  })
                  .catch(() => onError("Stop playback before deleting this music."));
              }
            }}
          >
            Delete
          </button>
        </article>
      ))}
    </section>
  );
}
