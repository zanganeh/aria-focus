import type { Update } from "@tauri-apps/plugin-updater";

type UpdateNoticeProps = {
  update: Update;
  installing: boolean;
  error: string | null;
  onInstall: () => void;
};

export function UpdateNotice({ update, installing, error, onInstall }: UpdateNoticeProps) {
  return (
    <section className="update-notice" aria-labelledby="update-notice-title" role="status">
      <div>
        <p className="eyebrow">Update available</p>
        <h2 id="update-notice-title">Aria Focus {update.version} is ready</h2>
        <p>{update.body || "A newer stable version is available."}</p>
        {error ? <p role="alert">{error}</p> : null}
      </div>
      <button type="button" className="primary" onClick={onInstall} disabled={installing}>
        {installing ? "Downloading…" : "Download and restart"}
      </button>
    </section>
  );
}
