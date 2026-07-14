import { useEffect, useState } from "react";
import {
  cancelRuntimeInstall,
  cancelStudioMusic,
  createStudioMusic,
  discardStudioDraft,
  getDraftPreviewState,
  getRuntimeInstall,
  getStudioCapability,
  listRecentStudioJobs,
  repairRuntime,
  pauseDraftPreview,
  regenerateStudioMusic,
  resumeDraftPreview,
  saveStudioDraft,
  startDraftPreview,
  startRuntimeInstall,
  stopDraftPreview,
} from "../lib/api";
import type { RuntimeInstall, StudioCapability, StudioJobSummary } from "../lib/types";

const NOTE_LIMIT = 240;

export function StudioPage({ onReturn }: { onReturn: () => void }) {
  const [capability, setCapability] = useState<StudioCapability | null>(null);
  const [items, setItems] = useState<StudioJobSummary[]>([]);
  const [note, setNote] = useState("");
  const [install, setInstall] = useState<RuntimeInstall | null>(null);
  const [activity, setActivity] = useState<
    "deep_work" | "motivation" | "creativity" | "learning" | "light_work"
  >("deep_work");
  const [style, setStyle] = useState<"ambient" | "gentle-piano" | "soft-electronic">("ambient");
  const [energy, setEnergy] = useState<"low" | "medium" | "high">("medium");
  const [duration, setDuration] = useState<90 | 180>(180);
  const [working, setWorking] = useState<StudioJobSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<{
    jobId: string | null;
    state: "stopped" | "playing" | "paused";
  }>({ jobId: null, state: "stopped" });
  const [handledJobIds, setHandledJobIds] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    void getStudioCapability()
      .then(setCapability)
      .catch(() =>
        setCapability({
          state: "needs_attention",
          detail: "Music Studio needs attention before it can be used.",
        }),
      );
    void listRecentStudioJobs()
      .then((jobs) => {
        setItems(jobs);
        setWorking(jobs.find((job) => job.status === "In progress") ?? null);
      })
      .catch(() => setItems([]));
    void getDraftPreviewState()
      .then((state) => {
        if (state !== "stopped") void stopDraftPreview();
        setPreview({ jobId: null, state: "stopped" });
      })
      .catch(() => setPreview({ jobId: null, state: "stopped" }));
    const poll = window.setInterval(
      () =>
        void listRecentStudioJobs()
          .then((jobs) => {
            setItems(jobs);
            setWorking(jobs.find((job) => job.status === "In progress") ?? null);
          })
          .catch(() => undefined),
      2500,
    );
    const installPoll = window.setInterval(() => {
      void getRuntimeInstall()
        .then((next) => {
          setInstall(next);
          if (next.status === "complete") void getStudioCapability().then(setCapability);
        })
        .catch(() => undefined);
    }, 500);
    void getRuntimeInstall()
      .then(setInstall)
      .catch(() => setInstall(null));
    return () => {
      window.clearInterval(poll);
      window.clearInterval(installPoll);
      void stopDraftPreview();
    };
  }, []);

  const returnToLibrary = () => {
    void stopDraftPreview().finally(onReturn);
  };

  if (!capability || capability.state === "checking") {
    return <section className="studio-page" aria-busy="true" aria-label="Checking Music Studio" />;
  }

  if (capability.state !== "ready") {
    const setup = capability.state === "setup_required";
    return (
      <section className="studio-page" aria-labelledby="studio-heading">
        <button className="back-action" type="button" onClick={returnToLibrary}>
          ← Library
        </button>
        <h2 id="studio-heading">Music Studio</h2>
        <p>
          {capability.detail ??
            (setup
              ? "Music Studio needs to be set up on this device."
              : "Music Studio is unavailable right now.")}
        </p>
        {setup && (
          <p>
            Your music and choices stay on this device.
            {capability.required_bytes
              ? ` About ${Math.ceil(capability.required_bytes / 1_000_000)} MB will be needed.`
              : ""}
          </p>
        )}
        {setup && (
          <>
            <p className="studio-muted">{install?.detail ?? "Music Studio is ready to install."}</p>
            {install?.status === "installing" &&
              install.total_bytes != null &&
              install.total_bytes > 0 && (
                <div className="studio-download-progress">
                  <progress
                    max={install.total_bytes}
                    value={install.downloaded_bytes ?? 0}
                    aria-label="Music Studio download progress"
                  />
                  <span>
                    {Math.floor(((install.downloaded_bytes ?? 0) / install.total_bytes) * 100)}%
                  </span>
                </div>
              )}
            {install?.status === "installing" ? (
              <button type="button" onClick={() => void cancelRuntimeInstall().then(setInstall)}>
                Cancel setup
              </button>
            ) : (
              <button
                className="primary"
                type="button"
                onClick={() =>
                  void startRuntimeInstall()
                    .then((result) => {
                      setInstall(result);
                      return getStudioCapability();
                    })
                    .then(setCapability)
                    .catch(() =>
                      setInstall({
                        status: "idle",
                        stage: "waiting",
                        detail: "Setup needs attention. Please try again.",
                      }),
                    )
                }
              >
                Install Music Studio
              </button>
            )}
          </>
        )}
        {!setup && capability.state === "needs_attention" && (
          <button
            type="button"
            onClick={() =>
              void repairRuntime()
                .then(setInstall)
                .then(() => getStudioCapability())
                .then(setCapability)
                .catch(() =>
                  setInstall({
                    status: "idle",
                    stage: "waiting",
                    detail: "Setup needs attention. Please try again.",
                  }),
                )
            }
          >
            Retry setup
          </button>
        )}
        <button type="button" onClick={returnToLibrary}>
          Return to Library
        </button>
      </section>
    );
  }

  return (
    <section className="studio-page" aria-labelledby="studio-heading">
      <button className="back-action" type="button" onClick={returnToLibrary}>
        ← Library
      </button>
      <p className="eyebrow">Music Studio</p>
      <h2 id="studio-heading">Create your focus music</h2>
      <p className="studio-muted">
        Choose what feels right. You can preview the result before saving.
      </p>
      <div className="studio-controls">
        <label>
          Focus type
          <select
            aria-label="Focus type"
            value={activity}
            onChange={(event) => setActivity(event.target.value as typeof activity)}
          >
            <option value="deep_work">Deep work</option>
            <option value="motivation">Motivation</option>
            <option value="light_work">Light work</option>
            <option value="learning">Learning</option>
            <option value="creativity">Creativity</option>
          </select>
        </label>
        <label>
          Sound style
          <select
            aria-label="Sound style"
            value={style}
            onChange={(event) => setStyle(event.target.value as typeof style)}
          >
            <option value="ambient">Ambient</option>
            <option value="gentle-piano">Gentle piano</option>
            <option value="soft-electronic">Soft electronic</option>
          </select>
        </label>
        <label>
          Energy
          <select
            aria-label="Energy"
            value={energy}
            onChange={(event) => setEnergy(event.target.value as typeof energy)}
          >
            <option value="low">Low</option>
            <option value="medium">Medium</option>
            <option value="high">High</option>
          </select>
        </label>
        <label>
          Length
          <select
            aria-label="Length"
            value={duration}
            onChange={(event) => setDuration(Number(event.target.value) as 90 | 180)}
          >
            <option value={90}>90 sec</option>
            <option value={180}>3 min</option>
          </select>
        </label>
      </div>
      <label className="studio-note">
        Anything else? <span>For example: “soft rain, no sudden changes”</span>
        <textarea
          aria-label="Anything else?"
          maxLength={NOTE_LIMIT}
          value={note}
          onChange={(event) => setNote(event.target.value)}
        />
        <small>
          {note.length}/{NOTE_LIMIT}
        </small>
      </label>
      <button
        type="button"
        className="primary"
        disabled={working !== null}
        onClick={() => {
          setError(null);
          void createStudioMusic({
            activity,
            sound_style_id: style,
            energy,
            duration_seconds: duration,
            note: note || null,
            parent_job_id: null,
          })
            .then((job) => {
              setWorking(job);
              setItems((old) => [job, ...old]);
            })
            .catch(() => setError("Your music could not be started. Please try again."));
        }}
      >
        {working ? "Creating music" : "Generate"}
      </button>
      {error && <p role="alert">{error}</p>}
      {working && (
        <p className="studio-muted">
          {working.stage === "preparing"
            ? "Preparing your music."
            : working.stage === "checking"
              ? "Checking your music."
              : "Creating your music."}{" "}
          You can keep using the app while it finishes.
        </p>
      )}
      {working && (
        <button
          type="button"
          onClick={() => {
            setError(null);
            void cancelStudioMusic(working.id)
              .then((job) => {
                setWorking(null);
                setItems((old) => old.map((item) => (item.id === job.id ? job : item)));
              })
              .catch(() => setError("Your music could not be cancelled right now."));
          }}
        >
          Cancel
        </button>
      )}
      {items.length > 0 && (
        <section className="studio-saved" aria-label="Recent Music Studio creations">
          <h3>Recent creations</h3>
          {items
            .filter((item) => !handledJobIds.has(item.id))
            .map((item) => (
              <div key={item.id}>
                <p>
                  <strong>{item.status}</strong> · {item.length_seconds < 120 ? "90 sec" : "3 min"}.{" "}
                  {item.safe_message ?? ""}
                </p>
                {item.can_preview &&
                  (preview.jobId === item.id && preview.state === "playing" ? (
                    <button
                      type="button"
                      onClick={() =>
                        void pauseDraftPreview()
                          .then(() => setPreview({ jobId: item.id, state: "paused" }))
                          .catch(() => setError("This preview could not be paused."))
                      }
                    >
                      Pause preview
                    </button>
                  ) : preview.jobId === item.id && preview.state === "paused" ? (
                    <button
                      type="button"
                      onClick={() =>
                        void resumeDraftPreview()
                          .then(() => setPreview({ jobId: item.id, state: "playing" }))
                          .catch(() => setError("This preview could not be resumed."))
                      }
                    >
                      Resume preview
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() =>
                        void startDraftPreview(item.id)
                          .then(() => setPreview({ jobId: item.id, state: "playing" }))
                          .catch(() => setError("This preview could not be started."))
                      }
                    >
                      Preview
                    </button>
                  ))}
                {preview.jobId === item.id && preview.state !== "stopped" && (
                  <button
                    type="button"
                    onClick={() =>
                      void stopDraftPreview()
                        .then(() => setPreview({ jobId: null, state: "stopped" }))
                        .catch(() => setError("This preview could not be stopped."))
                    }
                  >
                    Stop preview
                  </button>
                )}
                {item.can_save && (
                  <button
                    type="button"
                    onClick={() => {
                      const title = window.prompt("Name this music", "My focus music");
                      if (title) {
                        void saveStudioDraft(item.id, title)
                          .then(() => {
                            setPreview({ jobId: null, state: "stopped" });
                            setHandledJobIds((old) => new Set(old).add(item.id));
                          })
                          .catch(() =>
                            setError("This music could not be saved. Please try again."),
                          );
                      }
                    }}
                  >
                    Save to My Music
                  </button>
                )}
                {item.can_preview && (
                  <button
                    type="button"
                    onClick={() => {
                      setError(null);
                      void regenerateStudioMusic(item.id, {
                        activity,
                        sound_style_id: style,
                        energy,
                        duration_seconds: duration,
                        note: note || null,
                      })
                        .then((job) => {
                          setWorking(job);
                          setItems((old) => [job, ...old]);
                        })
                        .catch(() =>
                          setError("Your music could not be started. Please try again."),
                        );
                    }}
                  >
                    Generate another
                  </button>
                )}
                {item.can_discard && (
                  <button
                    type="button"
                    onClick={() => {
                      if (window.confirm("Discard this draft?")) {
                        void discardStudioDraft(item.id)
                          .then(() => {
                            if (preview.jobId === item.id) {
                              setPreview({ jobId: null, state: "stopped" });
                            }
                            setHandledJobIds((old) => new Set(old).add(item.id));
                            setItems((old) => old.filter((candidate) => candidate.id !== item.id));
                          })
                          .catch(() => setError("This draft could not be discarded."));
                      }
                    }}
                  >
                    Discard
                  </button>
                )}
              </div>
            ))}
        </section>
      )}
    </section>
  );
}
