import { useEffect, useMemo, useRef, useState } from "react";
import {
  activateCloudGeneration,
  cancelCloudGeneration,
  createCloudGeneration,
  deactivateCloudGeneration,
  estimateCloudGeneration,
  getActiveCloudGeneration,
  getCloudGeneration,
  getCloudGenerationItems,
  getCloudKeyStatus,
  listCloudModels,
  removeCloudKey,
  restoreCloudGeneration,
  saveCloudKey,
  startCloudGenerationPreview,
  stopDraftPreview,
} from "../lib/api";
import { listenCloudGenerationChanged } from "../lib/events";
import type {
  Activity,
  CloudBatchItem,
  CloudBatchSummary,
  CloudCostEstimate,
  CloudGenerationRequest,
  CloudKeyStatus,
  CloudModel,
} from "../lib/types";

type CloudPanelView = "create" | "settings";
type CreateStage = "define" | "estimate" | "generate" | "review";

interface CloudGenerationPanelProps {
  view?: CloudPanelView;
  onOpenSettings?: () => void;
  onOpenCreate?: () => void;
}

const ACTIVITIES: Array<[Activity, string]> = [
  ["deep_work", "Deep Work"],
  ["motivation", "Motivation"],
  ["creativity", "Creativity"],
  ["learning", "Learning"],
  ["light_work", "Light Work"],
];

const AUDIO_DEFAULT = "google/lyria-3-pro-preview";
const TEXT_DEFAULT = "google/gemini-2.5-flash";
const IMAGE_DEFAULT = "google/gemini-2.5-flash-image";

function dollars(microdollars: number) {
  const amount = microdollars / 1_000_000;
  if (amount === 0) return "$0.00";
  if (amount < 0.01) return `$${amount.toFixed(5)}`;
  if (amount < 1) return `$${amount.toFixed(4)}`;
  return `$${amount.toFixed(2)}`;
}

function friendlyError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  if (/401|key|credential/i.test(message))
    return "OpenRouter rejected the API key. Check it in Settings and try again.";
  if (/402|credit|budget/i.test(message))
    return "OpenRouter reports insufficient credits or the selected budget is too low.";
  if (/429|rate/i.test(message))
    return "OpenRouter is rate-limiting requests. Wait a moment and try again.";
  if (/model|404|unavailable/i.test(message))
    return "The selected model is unavailable or does not support this output.";
  if (/network|timeout|reach/i.test(message))
    return "OpenRouter could not be reached. Offline listening remains available.";
  return message;
}

function technicalFlags(item: CloudBatchItem): string[] {
  if (!item.validation_json) return [];
  try {
    const report = JSON.parse(item.validation_json) as {
      hard_rejections?: Array<{ code?: string; message?: string }>;
    };
    return (report.hard_rejections ?? []).map(
      (reason) => reason.message ?? reason.code ?? "Technical review required",
    );
  } catch {
    return ["Saved analyzer report could not be read."];
  }
}

function modelOptions(models: CloudModel[], modality: string, fallback: string) {
  const options = models.filter(
    (model) => supportsModality(model, modality) && hasPricingFor(model, modality),
  );
  if (models.length === 0 && !options.some((model) => model.id === fallback)) {
    options.unshift({
      id: fallback,
      name: null,
      description: null,
      input_modalities: [],
      output_modalities: [],
      supported_parameters: [],
      pricing: { prompt: null, completion: null, request: null, image: null },
      context_length: null,
      curated: true,
    });
  }
  return options;
}

function supportsModality(model: CloudModel, modality: string) {
  return model.output_modalities.includes(modality) || model.output_modalities.length === 0;
}

function hasPricingFor(model: CloudModel, modality: string) {
  if (modality === "text") {
    return !!(model.pricing.request || model.pricing.prompt || model.pricing.completion);
  }
  if (modality === "image") {
    return !!(model.pricing.request || model.pricing.image_output || model.pricing.image);
  }
  return !!(model.pricing.request || model.pricing.audio_output || model.pricing.audio);
}

function modelLabel(model: CloudModel, modality: string) {
  const price =
    modality === "audio"
      ? model.pricing.request
      : modality === "image"
        ? (model.pricing.image_output ?? model.pricing.request)
        : null;
  if (!price) return model.name ?? model.id;
  const amount = Number(price);
  return Number.isFinite(amount)
    ? `${model.name ?? model.id} · $${amount.toFixed(2)}/${modality === "audio" ? "track" : "image"}`
    : (model.name ?? model.id);
}

function compatibleModelId(models: CloudModel[], modality: string, preferred: string) {
  const preferredModel = models.find(
    (model) =>
      model.id === preferred && supportsModality(model, modality) && hasPricingFor(model, modality),
  );
  if (preferredModel) return preferredModel.id;
  return (
    models.find((model) => supportsModality(model, modality) && hasPricingFor(model, modality))
      ?.id ?? preferred
  );
}

export function CloudGenerationPanel({
  view = "create",
  onOpenSettings,
  onOpenCreate,
}: CloudGenerationPanelProps) {
  const [keyStatus, setKeyStatus] = useState<CloudKeyStatus | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [models, setModels] = useState<CloudModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelReload, setModelReload] = useState(0);
  const [activities, setActivities] = useState<Activity[]>([
    "deep_work",
    "motivation",
    "creativity",
    "learning",
    "light_work",
  ]);
  const [targetCount, setTargetCount] = useState(1);
  const [audioModel, setAudioModel] = useState(AUDIO_DEFAULT);
  const [textModel, setTextModel] = useState(TEXT_DEFAULT);
  const [imageModel, setImageModel] = useState(IMAGE_DEFAULT);
  const [refinePrompts, setRefinePrompts] = useState(true);
  const [generateCovers, setGenerateCovers] = useState(true);
  const [duration, setDuration] = useState(180);
  const [budget, setBudget] = useState(0.1);
  const [budgetEdited, setBudgetEdited] = useState(false);
  const [estimate, setEstimate] = useState<CloudCostEstimate | null>(null);
  const [estimateLoading, setEstimateLoading] = useState(false);
  const [batch, setBatch] = useState<CloudBatchSummary | null>(null);
  const [items, setItems] = useState<CloudBatchItem[]>([]);
  const [previewingItem, setPreviewingItem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [stage, setStage] = useState<CreateStage>("define");
  const [note, setNote] = useState("");
  const [reviewPage, setReviewPage] = useState(0);
  const cloudEventsConnected = useRef(false);

  // Two candidates keep the review action visible in the compact desktop window.
  // Pagination still exposes the complete batch without introducing page scroll.
  const reviewPageSize = 2;
  const reviewPageCount = Math.max(1, Math.ceil(items.length / reviewPageSize));
  const reviewItems = items.slice(
    reviewPage * reviewPageSize,
    reviewPage * reviewPageSize + reviewPageSize,
  );

  const request = useMemo<CloudGenerationRequest>(
    () => ({
      target_count: targetCount,
      activities,
      audio_model: audioModel,
      text_model: refinePrompts ? textModel : null,
      image_model: generateCovers ? imageModel : null,
      refine_prompts: refinePrompts,
      generate_covers: generateCovers,
      duration_seconds: duration,
      budget_microdollars: Math.max(0, Math.round(budget * 1_000_000)),
      note: note.trim() || null,
    }),
    [
      activities,
      audioModel,
      budget,
      duration,
      generateCovers,
      imageModel,
      refinePrompts,
      targetCount,
      textModel,
      note,
    ],
  );

  useEffect(() => {
    let active = true;
    void getCloudKeyStatus()
      .then((status) => {
        if (active) setKeyStatus(status);
        if ((status.configured || status.mock) && view === "create") {
          setModelsLoading(true);
          void listCloudModels()
            .then((next) => {
              if (!active) return;
              setModels(next);
              if (next.length === 0) {
                setError(
                  "OpenRouter returned no compatible priced models. Check the key or reload the model list.",
                );
              } else {
                setError(null);
              }
              setAudioModel((current) => compatibleModelId(next, "audio", current));
              setTextModel((current) => compatibleModelId(next, "text", current));
              setImageModel((current) => compatibleModelId(next, "image", current));
            })
            .catch((reason) => {
              if (!active) return;
              setModels([]);
              setError(friendlyError(reason));
            })
            .finally(() => active && setModelsLoading(false));
        }
      })
      .catch((reason) => active && setError(friendlyError(reason)));
    return () => {
      active = false;
    };
  }, [view, modelReload]);

  useEffect(() => {
    let active = true;
    void getActiveCloudGeneration()
      .then((next) => {
        if (!active || !next) return;
        setBatch(next);
        setStage(next.state === "validated" ? "review" : "generate");
        setReviewPage(0);
        void Promise.resolve(getCloudGenerationItems(next.batch_id))
          .then((nextItems) => {
            if (!active) return;
            const safeItems = nextItems ?? [];
            setItems(safeItems);
            setReviewPage((current) =>
              Math.min(current, Math.max(0, Math.ceil(safeItems.length / reviewPageSize) - 1)),
            );
          })
          .catch(() => undefined);
      })
      .catch((reason) => active && setError(friendlyError(reason)));
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void listenCloudGenerationChanged((event) => {
      if (!active) return;
      cloudEventsConnected.current = true;
      if (event.state === "validated") setStage("review");
      setBatch((current) => {
        if (!current || current.batch_id !== event.batchId) return current;
        return {
          ...current,
          state: event.state,
          target_count: event.targetCount,
          completed_count: event.completedCount,
          failed_count: event.failedCount,
          actual_microdollars: event.actualMicrodollars,
          error_message: event.errorMessage,
        };
      });
      void Promise.resolve(getCloudGenerationItems(event.batchId))
        .then((nextItems) => {
          if (!active) return;
          const safeItems = nextItems ?? [];
          setItems(safeItems);
          setReviewPage((current) =>
            Math.min(current, Math.max(0, Math.ceil(safeItems.length / reviewPageSize) - 1)),
          );
        })
        .catch(() => undefined);
      void Promise.resolve(getCloudGeneration(event.batchId))
        .then((next) => {
          if (!active || !next) return;
          setBatch(next);
          if (next.state === "validated") setStage("review");
        })
        .catch(() => undefined);
    })
      .then((cleanup) => {
        if (!active) cleanup();
        else {
          cloudEventsConnected.current = true;
          unlisten = cleanup;
        }
      })
      .catch(() => {
        // The command snapshots remain the compatibility fallback for older
        // preview shells that do not expose Tauri events.
      });
    return () => {
      active = false;
      cloudEventsConnected.current = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (
      !(keyStatus?.configured || keyStatus?.mock) ||
      view !== "create" ||
      batch ||
      modelsLoading ||
      models.length === 0
    )
      return;
    let active = true;
    setEstimateLoading(true);
    void estimateCloudGeneration(request)
      .then((next) => {
        if (!active) return;
        setError(null);
        setEstimate(next);
        setStage((current) => (current === "define" ? "estimate" : current));
        if (!budgetEdited) {
          const minimum = next.total_microdollars / 1_000_000;
          setBudget((current) => (Math.abs(current - minimum) < 0.000001 ? current : minimum));
        }
      })
      .catch((reason) => active && setError(friendlyError(reason)))
      .finally(() => active && setEstimateLoading(false));
    return () => {
      active = false;
    };
  }, [
    batch,
    budgetEdited,
    keyStatus?.configured,
    keyStatus?.mock,
    models.length,
    modelsLoading,
    request,
    view,
  ]);

  useEffect(() => {
    if (!batch || !["quoted", "authorized", "running", "validated"].includes(batch.state)) return;
    const poll = window.setInterval(() => {
      if (cloudEventsConnected.current) return;
      void Promise.resolve(getCloudGeneration(batch.batch_id))
        .then((next) => next && setBatch(next))
        .catch(() => undefined);
      void Promise.resolve(getCloudGenerationItems(batch.batch_id))
        .then((nextItems) => {
          const safeItems = nextItems ?? [];
          setItems(safeItems);
          setReviewPage((current) =>
            Math.min(current, Math.max(0, Math.ceil(safeItems.length / reviewPageSize) - 1)),
          );
        })
        .catch(() => undefined);
    }, 800);
    return () => window.clearInterval(poll);
  }, [batch]);

  const setKey = async () => {
    if (!keyInput.trim() || busy) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const status = await saveCloudKey(keyInput);
      setKeyStatus(status);
      setKeyInput("");
      setNotice("OpenRouter key validated and stored in your operating-system credential store.");
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!window.confirm("Remove the saved OpenRouter key from this device?")) return;
    setBusy(true);
    try {
      setKeyStatus(await removeCloudKey());
      setModels([]);
      setEstimate(null);
      setEstimateLoading(false);
      setNotice("OpenRouter key removed.");
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  };

  const start = async () => {
    if (!estimate || batch || busy) return;
    if (request.budget_microdollars < estimate.total_microdollars) {
      setError(`Increase the budget to at least ${dollars(estimate.total_microdollars)}.`);
      return;
    }
    if (
      !window.confirm(
        `Generate ${targetCount} track${targetCount === 1 ? "" : "s"} for up to ${dollars(estimate.total_microdollars)}?`,
      )
    )
      return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const next = await createCloudGeneration(request);
      setBatch(next);
      setItems([]);
      setReviewPage(0);
      setStage(next.state === "validated" ? "review" : "generate");
      setNotice(
        "Generation started. Your current library remains available while candidates are built.",
      );
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  };

  const cancel = async () => {
    setBusy(true);
    try {
      await cancelCloudGeneration();
      setNotice(
        "Cancellation requested. The current provider request will finish or time out safely.",
      );
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  };

  const startNewBatch = () => {
    setBatch(null);
    setItems([]);
    setReviewPage(0);
    setPreviewingItem(null);
    setEstimateLoading(false);
    setError(null);
    setNotice(null);
    setStage("define");
  };

  const preview = async (item: CloudBatchItem) => {
    setBusy(true);
    setError(null);
    try {
      if (previewingItem === item.item_id) {
        await stopDraftPreview();
        setPreviewingItem(null);
      } else {
        if (!batch) return;
        await startCloudGenerationPreview(batch.batch_id, item.item_id);
        setPreviewingItem(item.item_id);
        setNotice(`Previewing ${item.activity.replace(/_/g, " ")}.`);
      }
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  };

  const activate = async () => {
    if (!batch || batch.state !== "validated" || busy) return;
    if (!window.confirm("Save these validated tracks to the active music library?")) return;
    setBusy(true);
    setError(null);
    try {
      const next = await activateCloudGeneration(batch.batch_id);
      setBatch(next);
      setNotice("Tracks saved. They are now available for offline listening.");
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  };

  const restore = async () => {
    if (!window.confirm("Restore the previous activated cloud library?")) return;
    setBusy(true);
    try {
      await restoreCloudGeneration();
      setNotice("The previous cloud library is active again.");
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  };

  const deactivate = async () => {
    if (
      !window.confirm("Stop using the generated cloud library and return to the bundled library?")
    )
      return;
    setBusy(true);
    try {
      await deactivateCloudGeneration();
      setBatch(null);
      setItems([]);
      setNotice("The bundled library is active again. Generated files were kept for review.");
    } catch (reason) {
      setError(friendlyError(reason));
    } finally {
      setBusy(false);
    }
  };

  const keySection = (
    <div className="cloud-key-setup">
      {keyStatus?.mock ? (
        <>
          <strong>Test-only mock provider is active.</strong>
          <p className="studio-muted">
            This build uses local fixture audio and cover art. It never contacts OpenRouter or
            spends credits.
          </p>
        </>
      ) : !keyStatus?.configured ? (
        <>
          <label>
            OpenRouter API key
            <input
              type="password"
              autoComplete="off"
              value={keyInput}
              onChange={(event) => setKeyInput(event.target.value)}
              placeholder="Paste a temporary key"
            />
          </label>
          <p className="studio-muted">
            The key is sent only from this device to OpenRouter and is stored in the
            operating-system credential store. It is never saved in app preferences.
          </p>
          <button
            type="button"
            className="primary"
            disabled={busy || keyInput.trim().length < 16}
            onClick={() => void setKey()}
          >
            Validate and save key
          </button>
        </>
      ) : (
        <div className="cloud-key-status">
          <span>Key connected · ending in {keyStatus.masked_suffix ?? "••••"}</span>
          <button type="button" disabled={busy} onClick={() => void remove()}>
            Forget key
          </button>
        </div>
      )}
    </div>
  );

  if (view === "settings") {
    return (
      <section className="cloud-generation-panel" aria-labelledby="cloud-settings-heading">
        <div className="screen-heading">
          <p className="eyebrow">Connection</p>
          <h2 id="cloud-settings-heading">OpenRouter</h2>
          <p>Connect your own OpenRouter account to create music. Your key stays on this device.</p>
        </div>
        {error && (
          <p className="cloud-generation-message cloud-generation-error" role="alert">
            {error}
          </p>
        )}
        {notice && (
          <p className="cloud-generation-message" role="status">
            {notice}
          </p>
        )}
        {batch && (
          <div className="cloud-batch-status cloud-batch-status-banner" role="status">
            <strong>
              {batch.state === "validated" ? "Tracks ready to review" : `Generation ${batch.state}`}
            </strong>
            <span>
              {batch.completed_count}/{batch.target_count} complete
            </span>
            {batch.error_message && (
              <p className="cloud-generation-message cloud-generation-error">
                {friendlyError(batch.error_message)}
              </p>
            )}
            {onOpenCreate && (
              <button type="button" className="primary" onClick={onOpenCreate}>
                Continue in Create
              </button>
            )}
            {batch.activation_version && (
              <button type="button" disabled={busy} onClick={() => void deactivate()}>
                Deactivate generated library
              </button>
            )}
          </div>
        )}
        {keySection}
        <button type="button" onClick={onOpenCreate} disabled={!onOpenCreate}>
          Open Create
        </button>
      </section>
    );
  }

  const minimumBudget = estimate?.total_microdollars ?? 0;
  const budgetTooLow = !!estimate && request.budget_microdollars < minimumBudget;
  const audioOptions = modelOptions(models, "audio", AUDIO_DEFAULT);
  const textOptions = modelOptions(models, "text", TEXT_DEFAULT);
  const imageOptions = modelOptions(models, "image", IMAGE_DEFAULT);
  const hasAudioModel = audioOptions.some((model) => model.id === audioModel);
  const mockMode = keyStatus?.mock === true;
  const providerConfigured = keyStatus?.configured === true || mockMode;

  const stageTitle: Record<CreateStage, string> = {
    define: "Choose the sound",
    estimate: "Review the cost",
    generate: "Generation in progress",
    review: "Review candidates",
  };

  return (
    <section
      className="cloud-generation-panel"
      data-stage={stage}
      aria-labelledby="cloud-generation-heading"
    >
      <div className="screen-heading">
        <p className="eyebrow">Create music · {stageTitle[stage]}</p>
        <h2 id="cloud-generation-heading">Create your focus music</h2>
        <p>Choose the intent, confirm the live price, then preview before saving.</p>
      </div>
      {error && (
        <p className="cloud-generation-message cloud-generation-error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="cloud-generation-message" role="status">
          {notice}
        </p>
      )}
      {!providerConfigured ? (
        <div className="cloud-key-setup">
          <strong>OpenRouter is not connected yet.</strong>
          <p className="studio-muted">
            Set up your key in Settings before creating music. Normal listening stays offline.
          </p>
          <button
            type="button"
            className="primary"
            onClick={onOpenSettings}
            disabled={!onOpenSettings}
          >
            Set up OpenRouter in Settings
          </button>
        </div>
      ) : (
        <>
          <div className="cloud-key-status">
            <span>
              {mockMode
                ? "Test-only mock provider · no charges"
                : `OpenRouter connected · ending in ${keyStatus.masked_suffix ?? "••••"}`}
            </span>
            {!mockMode && (
              <button type="button" onClick={onOpenSettings} disabled={!onOpenSettings}>
                Manage connection
              </button>
            )}
          </div>
          <div className="cloud-stage-steps" aria-label="Music creation stages">
            {(["define", "estimate", "generate", "review"] as CreateStage[]).map((step) => (
              <button
                key={step}
                type="button"
                className={stage === step ? "selected" : ""}
                disabled={step === "generate" || step === "review" ? !batch : false}
                onClick={() => setStage(step)}
              >
                {stageTitle[step]}
              </button>
            ))}
          </div>
          {modelsLoading && (
            <p className="studio-muted">Loading compatible models and their live prices…</p>
          )}
          {estimateLoading && !modelsLoading && (
            <p className="studio-muted">Checking the selected models and live price…</p>
          )}
          {!modelsLoading && models.length === 0 && (
            <p className="cloud-generation-message cloud-generation-error" role="alert">
              {error ??
                "No compatible priced models were returned. Check the OpenRouter key and reload."}
            </p>
          )}
          {!modelsLoading && models.length === 0 && (
            <button type="button" onClick={() => setModelReload((current) => current + 1)}>
              Reload compatible models
            </button>
          )}
          {!modelsLoading && models.length > 0 && !hasAudioModel && (
            <p className="cloud-generation-message cloud-generation-error" role="alert">
              No priced audio model is available for this key. Choose another OpenRouter account.
            </p>
          )}

          {stage === "define" && (
            <div className="cloud-stage-content">
              <div className="cloud-generation-controls">
                <label>
                  Audio model
                  <select
                    value={audioModel}
                    onChange={(event) => {
                      setAudioModel(event.target.value);
                      setStage("define");
                    }}
                  >
                    {audioOptions.map((model) => (
                      <option key={model.id} value={model.id}>
                        {modelLabel(model, "audio")}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  Tracks to create
                  <select
                    value={targetCount}
                    onChange={(event) => {
                      setTargetCount(Number(event.target.value));
                      setStage("define");
                    }}
                  >
                    <option value={1}>1 track</option>
                    <option value={5}>5 tracks</option>
                    <option value={10}>10 tracks</option>
                    <option value={25}>25 tracks</option>
                    <option value={100}>100 tracks</option>
                  </select>
                </label>
                <label>
                  Duration per track
                  <select
                    value={duration}
                    onChange={(event) => {
                      setDuration(Number(event.target.value));
                      setStage("define");
                    }}
                  >
                    <option value={90}>90 seconds</option>
                    <option value={180}>3 minutes</option>
                    <option value={300}>5 minutes</option>
                  </select>
                </label>
              </div>
              <fieldset>
                <legend>Activities</legend>
                <div className="cloud-activity-options">
                  {ACTIVITIES.map(([id, label]) => (
                    <label key={id}>
                      <input
                        type="checkbox"
                        checked={activities.includes(id)}
                        onChange={(event) =>
                          setActivities((current) =>
                            event.target.checked
                              ? [...current, id]
                              : current.filter((value) => value !== id),
                          )
                        }
                      />
                      {label}
                    </label>
                  ))}
                </div>
              </fieldset>
              <label>
                Optional direction
                <textarea
                  value={note}
                  maxLength={240}
                  rows={2}
                  placeholder="For example: warm, deep, instrumental focus with an immediate pulse."
                  onChange={(event) => setNote(event.target.value)}
                />
              </label>
              <details>
                <summary>Advanced models and cover art</summary>
                <div className="cloud-advanced-grid">
                  <label>
                    Prompt model
                    <select
                      value={textModel}
                      onChange={(event) => setTextModel(event.target.value)}
                    >
                      {textOptions.map((model) => (
                        <option key={model.id} value={model.id}>
                          {modelLabel(model, "text")}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    Cover model
                    <select
                      value={imageModel}
                      onChange={(event) => setImageModel(event.target.value)}
                    >
                      {imageOptions.map((model) => (
                        <option key={model.id} value={model.id}>
                          {modelLabel(model, "image")}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
                <label>
                  <input
                    type="checkbox"
                    checked={refinePrompts}
                    onChange={(event) => setRefinePrompts(event.target.checked)}
                  />{" "}
                  Refine the structured prompt
                </label>
                <label>
                  <input
                    type="checkbox"
                    checked={generateCovers}
                    onChange={(event) => setGenerateCovers(event.target.checked)}
                  />{" "}
                  Generate related cover art
                </label>
              </details>
              <button
                type="button"
                className="primary"
                disabled={!estimate || estimateLoading || activities.length === 0}
                onClick={() => setStage("estimate")}
              >
                Review estimate
              </button>
            </div>
          )}

          {stage === "estimate" && (
            <div className="cloud-stage-content">
              <div className="cloud-cost-estimate" aria-label="Generation cost estimate">
                <strong>Minimum budget: {dollars(minimumBudget)}</strong>
                <span>
                  {targetCount} track{targetCount === 1 ? "" : "s"} · {duration}s each · Audio{" "}
                  {dollars(estimate?.audio_microdollars ?? 0)} · Prompt{" "}
                  {dollars(estimate?.text_microdollars ?? 0)} · Covers{" "}
                  {dollars(estimate?.image_microdollars ?? 0)}
                </span>
                <small>{estimate?.pricing_source ?? "Waiting for live model pricing"}</small>
              </div>
              <label>
                Maximum budget (USD)
                <input
                  type="number"
                  min={
                    minimumBudget ? (minimumBudget / 1_000_000).toFixed(6) : mockMode ? "0" : "0.01"
                  }
                  step="0.01"
                  value={budget}
                  onChange={(event) => {
                    const next = Number(event.target.value);
                    setBudget(Number.isFinite(next) && next >= 0 ? next : 0);
                    setBudgetEdited(true);
                  }}
                />
              </label>
              <p className="studio-muted">
                The minimum is recalculated from the selected models and number of tracks. Nothing
                is charged until you confirm.
              </p>
              {budgetTooLow && (
                <p className="cloud-generation-message cloud-generation-error" role="alert">
                  Increase the budget to at least {dollars(minimumBudget)} before generating.
                </p>
              )}
              <div className="cloud-generation-actions">
                <button type="button" onClick={() => setStage("define")}>
                  Edit choices
                </button>
                <button
                  type="button"
                  className="primary"
                  disabled={
                    busy ||
                    !!batch ||
                    modelsLoading ||
                    !hasAudioModel ||
                    !estimate ||
                    budgetTooLow ||
                    activities.length === 0
                  }
                  onClick={() => void start()}
                >
                  {targetCount === 100
                    ? "Generate replacement library"
                    : `Generate ${targetCount} track${targetCount === 1 ? "" : "s"}`}
                </button>
              </div>
            </div>
          )}

          {stage === "generate" && batch && (
            <div className="cloud-stage-content cloud-batch-status" role="status">
              <strong>
                {batch.state === "validated"
                  ? "Tracks ready to review"
                  : `Generation ${batch.state}`}
              </strong>
              <span>{`${batch.completed_count}/${batch.target_count} complete`}</span>
              <span>Actual {dollars(batch.actual_microdollars)}</span>
              {batch.error_message && (
                <p className="cloud-generation-message cloud-generation-error">
                  {friendlyError(batch.error_message)}
                </p>
              )}
              {batch.state === "validated" ? (
                <button type="button" className="primary" onClick={() => setStage("review")}>
                  Review candidates
                </button>
              ) : ["failed", "cancelled"].includes(batch.state) ? (
                <button type="button" className="primary" onClick={startNewBatch}>
                  Start a new batch
                </button>
              ) : (
                <button
                  type="button"
                  disabled={busy || !["running", "authorized"].includes(batch.state)}
                  onClick={() => void cancel()}
                >
                  Cancel batch
                </button>
              )}
            </div>
          )}

          {stage === "review" && batch && (
            <div className="cloud-stage-content cloud-batch-status" role="status">
              <strong>Tracks ready to review</strong>
              <span>{`${batch.completed_count}/${batch.target_count} complete`}</span>
              <span>Actual {dollars(batch.actual_microdollars)}</span>
              {items.length > 0 ? (
                <>
                  <ul>
                    {reviewItems.map((item) => (
                      <li key={item.item_id}>
                        <div className="cloud-candidate-info">
                          {item.cover_art ? (
                            <img
                              className="cloud-candidate-cover"
                              src={item.cover_art}
                              alt={`Generated cover for ${item.activity.replace(/_/g, " ")} track ${item.ordinal + 1}`}
                              decoding="async"
                            />
                          ) : (
                            <span
                              className="cloud-candidate-cover cloud-candidate-cover--empty"
                              aria-hidden="true"
                            />
                          )}
                          <span>
                            <strong>
                              {item.activity.replace(/_/g, " ")} track {item.ordinal + 1}
                            </strong>
                            <small>{item.state}</small>
                          </span>
                        </div>
                        {item.state === "validated" && (
                          <button type="button" disabled={busy} onClick={() => void preview(item)}>
                            {previewingItem === item.item_id ? "Stop preview" : "Preview"}
                          </button>
                        )}
                        {item.error_message && <small>{friendlyError(item.error_message)}</small>}
                        {technicalFlags(item).length > 0 && (
                          <small className="cloud-generation-warning" role="alert">
                            Technical review required: {technicalFlags(item).join("; ")}
                          </small>
                        )}
                        <details>
                          <summary>View saved prompt</summary>
                          <p className="cloud-saved-prompt">
                            {item.refined_prompt ?? item.prompt_json}
                          </p>
                        </details>
                      </li>
                    ))}
                  </ul>
                  {reviewPageCount > 1 && (
                    <div className="review-pagination" aria-label="Candidate pages">
                      <button
                        type="button"
                        disabled={reviewPage === 0}
                        onClick={() => setReviewPage((current) => Math.max(0, current - 1))}
                      >
                        Previous
                      </button>
                      <span>
                        Page {reviewPage + 1} of {reviewPageCount}
                      </span>
                      <button
                        type="button"
                        disabled={reviewPage >= reviewPageCount - 1}
                        onClick={() =>
                          setReviewPage((current) => Math.min(reviewPageCount - 1, current + 1))
                        }
                      >
                        Next
                      </button>
                    </div>
                  )}
                </>
              ) : (
                <p className="studio-muted">The candidates are still being checked.</p>
              )}
              {batch.state === "validated" && (
                <button
                  type="button"
                  className="primary"
                  disabled={busy}
                  onClick={() => void activate()}
                >
                  Save and activate tracks
                </button>
              )}
              {batch.activation_version && (
                <button type="button" onClick={() => void restore()}>
                  Restore previous cloud library
                </button>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
