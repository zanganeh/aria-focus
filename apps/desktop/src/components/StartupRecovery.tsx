import type { StartupHealth } from "../lib/types";

interface Props {
  health: StartupHealth;
  busy: boolean;
  retryError: string | null;
  onRetry: () => void;
}

export function StartupRecovery({ health, busy, retryError, onRetry }: Props) {
  const migrationUnavailable =
    health.migration_status === "failed" || health.migration_status === "conflict";
  if (health.core_ready && health.packs_ready && !migrationUnavailable) return null;
  return (
    <section className="startup-recovery" aria-labelledby="startup-recovery-title" role="status">
      <h2 id="startup-recovery-title">Some offline services are unavailable</h2>
      {migrationUnavailable && (
        <p>
          <strong>Existing ADHD Music data could not be migrated safely.</strong>{" "}
          {health.migration_error}
        </p>
      )}
      {!health.core_ready && (
        <p>
          <strong>Session and audio controls are unavailable.</strong> {health.core_error}
        </p>
      )}
      {!health.packs_ready && (
        <p>
          <strong>Installed content packs are unavailable.</strong> {health.packs_error}
        </p>
      )}
      {retryError && <p role="alert">Retry could not complete: {retryError}</p>}
      <button type="button" onClick={onRetry} disabled={busy} aria-busy={busy}>
        {busy ? "Retrying startup…" : "Retry startup"}
      </button>
    </section>
  );
}
