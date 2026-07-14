import { useEffect, useState } from "react";
import { chooseAndImportContentPack, listContentPacks } from "../lib/api";
import type { ContentPackSummary } from "../lib/types";

interface Props {
  onCatalogueChange?: () => void;
  disabled?: boolean;
}

export function ContentPacks({ onCatalogueChange, disabled = false }: Props) {
  const [packs, setPacks] = useState<ContentPackSummary[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void listContentPacks()
      .then((installed) => {
        if (active) setPacks(installed);
      })
      .catch((reason: unknown) => {
        if (active) setError(toMessage(reason));
      });
    return () => {
      active = false;
    };
  }, []);

  async function importPack() {
    if (disabled || busy) return;
    setBusy(true);
    setError(null);
    setSuccess(null);
    try {
      const installed = await chooseAndImportContentPack();
      if (installed === null) return;
      setPacks((current) => [...current.filter((pack) => pack.id !== installed.id), installed]);
      setSuccess(`${installed.title} was imported and validated.`);
      onCatalogueChange?.();
    } catch (reason) {
      setError(toMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="content-packs" aria-labelledby="content-packs-title">
      <div className="content-packs-heading">
        <div>
          <h2 id="content-packs-title">Installed content packs</h2>
          <p>
            Published-validated packs are integrity-checked, but only items that meet the current
            activity and continuous-playback contract are eligible for offline playback; a validated
            pack may still have no eligible playable item.
          </p>
        </div>
        <button type="button" onClick={() => void importPack()} disabled={disabled || busy}>
          {busy ? "Importing…" : "Import pack"}
        </button>
      </div>
      {error && (
        <p className="content-message error" role="alert">
          Import or integrity check failed: {error}
        </p>
      )}
      {success && (
        <p className="content-message" role="status">
          {success}
        </p>
      )}
      {packs.length === 0 ? (
        <p className="empty-packs">No validated content packs installed.</p>
      ) : (
        <ul className="pack-list">
          {packs.map((pack) => (
            <li key={pack.id}>
              <span>
                {pack.title}
                {pack.status === "owner_waived_bundled_private_beta" && (
                  <> · Private beta / owner waived</>
                )}
              </span>
              <span>
                v{pack.version} · {pack.item_count} {pack.item_count === 1 ? "item" : "items"}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function toMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}
