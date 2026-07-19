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
import type {
  RuntimeInstall,
  StudioCapability,
  StudioHardwareInfo,
  StudioJobSummary,
  StudioRequirements,
} from "../lib/types";

const DETAILS_LIMIT = 500;
const CREATIVE_DIRECTION_LIMIT = 2_000;

const BYTES_PER_GIB = 1024 * 1024 * 1024;
const BYTES_PER_GB = 1000 * 1000 * 1000;

function formatGib(bytes: number | null | undefined): string {
  if (bytes == null) return "unknown";
  const gib = bytes / BYTES_PER_GIB;
  return `${gib >= 1 ? gib.toFixed(0) : gib.toFixed(1)} GiB`;
}

function formatGb(bytes: number | null | undefined): string {
  if (bytes == null) return "unknown";
  return `${Math.ceil(bytes / BYTES_PER_GB)} GB`;
}

function cudaLabel(cuda: boolean | null | undefined): string {
  if (cuda === true) return "Yes";
  if (cuda === false) return "Not detected";
  return "unknown";
}

function buildStudioPromptPreview({
  activity,
  genre,
  energy,
  tempo,
  mood,
  instruments,
  details,
  creativeDirection,
}: {
  activity: string;
  genre: string;
  energy: string;
  tempo: number | null;
  mood: string | null;
  instruments: string[];
  details: string;
  creativeDirection: string;
}): string {
  const parts = [
    "instrumental music only",
    `activity: ${activity}`,
    `genre: ${genre}`,
    `energy: ${energy}`,
  ];
  if (tempo != null) parts.push(`tempo: ${tempo} BPM`);
  if (mood) parts.push(`mood: ${mood}`);
  if (instruments.length > 0) parts.push(`instruments: ${instruments.join(", ")}`);
  if (details) parts.push(`details: ${details}`);
  if (creativeDirection) parts.push(`creative direction: ${creativeDirection}`);
  return parts.join("; ");
}

function StudioRequirementsPanel({
  hardware,
  requirements,
  freeBytes,
}: {
  hardware: StudioHardwareInfo | null | undefined;
  requirements: StudioRequirements | null | undefined;
  freeBytes: number | null | undefined;
}) {
  if (!requirements) return null;
  return (
    <dl className="studio-requirements" role="group" aria-label="Music Studio requirements">
      <div>
        <dt>Minimum</dt>
        <dd>
          Windows 11 x64 ({requirements.architecture}), at least{" "}
          {formatGib(requirements.min_memory_bytes)} RAM, an NVIDIA CUDA GPU with at least{" "}
          {formatGib(requirements.min_vram_bytes)} VRAM, and about{" "}
          {formatGb(requirements.min_free_disk_bytes)} free disk.
        </dd>
      </div>
      <div>
        <dt>Detected</dt>
        <dd>
          Architecture: {hardware?.architecture ?? "unknown"}; RAM:{" "}
          {formatGib(hardware?.memory_bytes)}; GPU: {hardware?.accelerator ?? "unknown"} (
          {cudaLabel(hardware?.cuda)}); VRAM: {formatGib(hardware?.vram_bytes)}; free disk:{" "}
          {formatGb(freeBytes)}.
        </dd>
      </div>
    </dl>
  );
}

export function StudioPage({ onReturn }: { onReturn: () => void }) {
  const [capability, setCapability] = useState<StudioCapability | null>(null);
  const [items, setItems] = useState<StudioJobSummary[]>([]);
  const [install, setInstall] = useState<RuntimeInstall | null>(null);
  const [activity, setActivity] = useState<
    "deep_work" | "motivation" | "creativity" | "learning" | "light_work"
  >("deep_work");
  const [genre, setGenre] = useState("ambient");
  const [mood, setMood] = useState("focused");
  const [energy, setEnergy] = useState<"low" | "medium" | "high">("medium");
  const [tempo, setTempo] = useState<70 | 90 | 110 | 130 | 150>(90);
  const [duration, setDuration] = useState<90 | 180>(180);
  const [details, setDetails] = useState("");
  const [moreOpen, setMoreOpen] = useState(false);
  const [instruments, setInstruments] = useState<string[]>([]);
  const [creativeDirection, setCreativeDirection] = useState("");
  const [working, setWorking] = useState<StudioJobSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<{
    jobId: string | null;
    state: "stopped" | "playing" | "paused";
  }>({ jobId: null, state: "stopped" });
  const [handledJobIds, setHandledJobIds] = useState<Set<string>>(() => new Set());

  const studioRequest = (parent_job_id?: string | null) => ({
    activity,
    genre_id: genre,
    sound_style_id: genre,
    mood_id: mood || null,
    energy,
    tempo_bpm: tempo,
    duration_seconds: duration,
    instrument_ids: instruments,
    additional_details: details || null,
    creative_direction: creativeDirection || null,
    ...(parent_job_id === undefined ? {} : { parent_job_id }),
  });
  const promptPreview = buildStudioPromptPreview({
    activity,
    genre,
    energy,
    tempo,
    mood: mood || null,
    instruments,
    details,
    creativeDirection,
  });

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
        <p className="studio-muted">
          The packaged runtime bundles its own Python, pinned source, dependencies, and model
          snapshots. You do not need to install Python, uv, Git, FFmpeg, or model weights
          separately. One-time setup needs internet; after it completes, generation runs offline. If
          setup is interrupted, choose Set up Music Studio again to resume; if the installed runtime
          needs repair, use Retry setup.
        </p>
        <StudioRequirementsPanel
          hardware={capability.hardware}
          requirements={capability.requirements}
          freeBytes={capability.free_bytes}
        />
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
                Set up Music Studio
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
        Simple choices are already complete. Choose the basics and generate; More is optional if you
        want to steer the arrangement.
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
          Genre
          <select
            aria-label="Genre"
            value={genre}
            onChange={(event) => setGenre(event.target.value)}
          >
            <option value="ambient">Ambient</option>
            <option value="piano">Piano</option>
            <option value="electronic">Electronic</option>
            <option value="lofi">Lo-fi</option>
            <option value="acoustic">Acoustic</option>
          </select>
        </label>
        <label>
          Mood
          <select aria-label="Mood" value={mood} onChange={(event) => setMood(event.target.value)}>
            <option value="focused">Focused</option>
            <option value="calm">Calm</option>
            <option value="warm">Warm</option>
            <option value="uplifting">Uplifting</option>
            <option value="cinematic">Cinematic</option>
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
          Speed / tempo
          <select
            aria-label="Speed / tempo"
            value={tempo}
            onChange={(event) => setTempo(Number(event.target.value) as typeof tempo)}
          >
            <option value={70}>Slow · 70 BPM</option>
            <option value={90}>Steady · 90 BPM</option>
            <option value={110}>Moderate · 110 BPM</option>
            <option value={130}>Brisk · 130 BPM</option>
            <option value={150}>Fast · 150 BPM</option>
          </select>
        </label>
        <label>
          Duration
          <select
            aria-label="Duration"
            value={duration}
            onChange={(event) => setDuration(Number(event.target.value) as 90 | 180)}
          >
            <option value={90}>90 sec</option>
            <option value={180}>3 min</option>
          </select>
        </label>
      </div>
      <label className="studio-note">
        Details{" "}
        <span>Optional plain-language guidance, such as “soft rain, no sudden changes”</span>
        <textarea
          aria-label="Details"
          maxLength={DETAILS_LIMIT}
          value={details}
          onChange={(event) => setDetails(event.target.value)}
        />
        <small>
          {details.length}/{DETAILS_LIMIT}
        </small>
      </label>
      <section className="studio-more" aria-label="More Music Studio controls">
        <button
          type="button"
          className="studio-more-toggle"
          aria-expanded={moreOpen}
          onClick={() => setMoreOpen((open) => !open)}
        >
          <span>More</span>
          <small>Optional instruments and fuller creative direction</small>
          <span aria-hidden="true">{moreOpen ? "−" : "+"}</span>
        </button>
        {moreOpen && (
          <div className="studio-more-panel">
            <fieldset>
              <legend>Instruments</legend>
              <div className="studio-instruments">
                {[
                  ["piano", "Piano"],
                  ["synth", "Synth"],
                  ["strings", "Strings"],
                  ["guitar", "Guitar"],
                  ["percussion", "Percussion"],
                ].map(([id, label]) => (
                  <label key={id}>
                    <input
                      type="checkbox"
                      checked={instruments.includes(id)}
                      onChange={(event) =>
                        setInstruments((current) =>
                          event.target.checked
                            ? [...current, id].slice(0, 5)
                            : current.filter((instrument) => instrument !== id),
                        )
                      }
                    />
                    {label}
                  </label>
                ))}
              </div>
            </fieldset>
            <label className="studio-note">
              Fuller creative direction
              <textarea
                aria-label="Fuller creative direction"
                maxLength={CREATIVE_DIRECTION_LIMIT}
                value={creativeDirection}
                onChange={(event) => setCreativeDirection(event.target.value)}
              />
              <small>
                {creativeDirection.length}/{CREATIVE_DIRECTION_LIMIT}
              </small>
            </label>
            <div className="studio-prompt-preview">
              <p className="studio-prompt-label">Full prompt preview</p>
              <p className="studio-muted">
                This is the deterministic local prompt Music Studio will use. It stays on this
                device.
              </p>
              <pre aria-label="Full prompt preview">{promptPreview}</pre>
            </div>
          </div>
        )}
      </section>
      <button
        type="button"
        className="primary"
        disabled={working !== null}
        onClick={() => {
          setError(null);
          void createStudioMusic(studioRequest(null))
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
                {item.creative_prompt && (
                  <details className="studio-prompt-card">
                    <summary>Generated prompt</summary>
                    <p className="studio-prompt-label">Positive prompt</p>
                    <pre>{item.creative_prompt}</pre>
                    {item.locked_negative_prompt && (
                      <>
                        <p className="studio-prompt-label">Locked negative prompt</p>
                        <pre>{item.locked_negative_prompt}</pre>
                      </>
                    )}
                  </details>
                )}
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
                      void regenerateStudioMusic(item.id, studioRequest())
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
