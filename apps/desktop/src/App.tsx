import { useCallback, useEffect, useRef, useState } from "react";
import { ActivitySelector } from "./components/ActivitySelector";
import { ActivityArtwork } from "./components/ActivityArtwork";
import { AdhdModeToggle } from "./components/AdhdModeToggle";
import { GenreSelector } from "./components/GenreSelector";
import { MoodSelector } from "./components/MoodSelector";
import { ContentPacks } from "./components/ContentPacks";
import { Disclaimer } from "./components/Disclaimer";
import { ErrorBanner } from "./components/ErrorBanner";
import { IntensitySelector } from "./components/IntensitySelector";
import { MasterVolume } from "./components/MasterVolume";
import { SessionTimer } from "./components/SessionTimer";
import { SessionTypeSelector } from "./components/SessionTypeSelector";
import { TransportControls } from "./components/TransportControls";
import { StartupRecovery } from "./components/StartupRecovery";
import { QuarantinedReview } from "./components/QuarantinedReview";
import { FocusView } from "./components/FocusView";
import { Onboarding } from "./components/Onboarding";
import { FavoritesLibrary } from "./components/FavoritesLibrary";
import { RecentSessions } from "./components/RecentSessions";
import { AppIcon } from "./components/AppIcon";
import { StudioLibraryCard } from "./components/StudioLibraryCard";
import { CloudGenerationPanel } from "./components/CloudGenerationPanel";
import { MyMusicLibrary } from "./components/MyMusicLibrary";
import { BrandMark } from "./components/BrandMark";
import { LaunchScreen } from "./components/LaunchScreen";
import { AboutAriaFocus } from "./components/AboutAriaFocus";
import { UpdateNotice } from "./components/UpdateNotice";
import {
  getActivityGenres,
  getCurrentSource,
  getProvenance,
  getStartupHealth,
  retryStartup,
  setActivityGenre,
  getActivityMoods,
  setActivityMood,
  listReviewCandidates,
  startReviewCandidate,
  nextTrack,
  previousTrack,
  resetSessionTimer,
  completeOnboarding,
  getOnboardingPreferences,
  listRecentSessions,
} from "./lib/api";
import { ACTIVITY_COPY } from "./lib/activities";
import { useSession } from "./hooks/useSession";
import { useFocusedWindowTransportKeys } from "./hooks/useFocusedWindowTransportKeys";
import { findAvailableUpdate, installAndRelaunch } from "./lib/updater";
import { listenPlaybackChanged } from "./lib/events";
import type {
  ActivityGenreState,
  ActivityMoodState,
  Activity,
  CurrentSource,
  Provenance,
  StartupHealth,
  ReviewCandidate,
  SessionHistoryRecord,
} from "./lib/types";
import type { Update } from "@tauri-apps/plugin-updater";

type AppPage = "home" | "library" | "history" | "settings" | "review" | "studio";
type SettingsSection = "sound" | "focus" | "connection" | "help";
type LibrarySection = "overview" | "my_music" | "favorites" | "packs";

export default function App() {
  const session = useSession();
  const [provenance, setProvenance] = useState<Provenance | null>(null);
  const [source, setSource] = useState<CurrentSource | null>(null);
  const [genres, setGenres] = useState<ActivityGenreState | null>(null);
  const [moods, setMoods] = useState<ActivityMoodState | null>(null);
  const [catalogueRevision, setCatalogueRevision] = useState(0);
  const [favoritesRevision] = useState(0);
  const [contentPacksRevision, setContentPacksRevision] = useState(0);
  const [startupHealth, setStartupHealth] = useState<StartupHealth | null>(null);
  const [retryingStartup, setRetryingStartup] = useState(false);
  const [reviewCandidates, setReviewCandidates] = useState<ReviewCandidate[]>([]);
  const [reviewCandidatesLoaded, setReviewCandidatesLoaded] = useState(false);
  const [startupRetryError, setStartupRetryError] = useState<string | null>(null);
  const [focusView, setFocusView] = useState(false);
  const [expandedPlayer, setExpandedPlayer] = useState(false);
  const [page, setPage] = useState<AppPage>("home");
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("sound");
  const [librarySection, setLibrarySection] = useState<LibrarySection>("overview");
  const [navigationPending, setNavigationPending] = useState(false);
  const [activityPending, setActivityPending] = useState(false);
  const [pendingActivity, setPendingActivity] = useState<Activity | null>(null);
  const [onboardingComplete, setOnboardingComplete] = useState<boolean | null>(null);
  const [onboardingLoadError, setOnboardingLoadError] = useState<string | null>(null);
  const [recentSessions, setRecentSessions] = useState<SessionHistoryRecord[]>([]);
  const [availableUpdate, setAvailableUpdate] = useState<Update | null>(null);
  const [installingUpdate, setInstallingUpdate] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const previousStatus = useRef<string | null>(null);
  const focusEntryControl = useRef<HTMLButtonElement>(null);
  const scrollRegion = useRef<HTMLDivElement>(null);
  const retryInFlight = useRef(false);
  const healthRequest = useRef(0);
  const updateCheckStarted = useRef(false);
  const previousTransportActive = useRef(false);
  const navigationFromItem = useRef<string | null>(null);
  const navigationTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);
  const loadOnboardingPreferences = useCallback(async () => {
    setOnboardingComplete(null);
    setOnboardingLoadError(null);
    try {
      const preferences = await getOnboardingPreferences();
      setOnboardingComplete(preferences.completed);
    } catch (error) {
      setOnboardingLoadError(error instanceof Error ? error.message : String(error));
    }
  }, []);

  useEffect(() => {
    void loadOnboardingPreferences();
  }, [loadOnboardingPreferences]);

  useEffect(() => {
    if (updateCheckStarted.current) return;
    updateCheckStarted.current = true;
    let active = true;
    void findAvailableUpdate().then((update) => {
      if (active) setAvailableUpdate(update);
    });
    return () => {
      active = false;
    };
  }, []);

  const installUpdate = async () => {
    if (!availableUpdate || installingUpdate) return;
    setInstallingUpdate(true);
    setUpdateError(null);
    try {
      await installAndRelaunch(availableUpdate);
    } catch (error) {
      setUpdateError(
        `The update could not be installed: ${error instanceof Error ? error.message : String(error)}`,
      );
      setInstallingUpdate(false);
    }
  };

  const updateNotice = availableUpdate ? (
    <UpdateNotice
      update={availableUpdate}
      installing={installingUpdate}
      error={updateError}
      onInstall={() => void installUpdate()}
    />
  ) : null;

  useEffect(() => {
    const request = ++healthRequest.current;
    void getStartupHealth()
      .then((health) => {
        if (request === healthRequest.current) setStartupHealth(health);
      })
      .catch(() => {
        // The native command itself is unavailable; retain the existing UI rather than claiming recovery.
      });
  }, []);
  useEffect(() => {
    void listReviewCandidates()
      .then(setReviewCandidates)
      .catch(() => setReviewCandidates([]))
      .finally(() => setReviewCandidatesLoaded(true));
  }, []);

  const retryStartupServices = async () => {
    if (retryInFlight.current) return;
    retryInFlight.current = true;
    setRetryingStartup(true);
    setStartupRetryError(null);
    try {
      const health = await retryStartup();
      setStartupHealth(health);
      if (health.core_ready) {
        await session.refresh();
        session.clearSessionLoadError();
      }
      if (health.packs_ready) {
        setCatalogueRevision((revision) => revision + 1);
        setContentPacksRevision((revision) => revision + 1);
      }
    } catch (error) {
      setStartupRetryError(error instanceof Error ? error.message : String(error));
    } finally {
      retryInFlight.current = false;
      setRetryingStartup(false);
    }
  };

  useEffect(() => {
    void getProvenance()
      .then(setProvenance)
      .catch(() => setProvenance(null));
  }, []);

  const status = session.snapshot?.status ?? "idle";
  const transportActive = status === "playing" || status === "paused";
  const refreshSession = session.refresh;
  const reportSessionError = session.reportError;
  const activity = session.snapshot?.activity ?? "deep_work";
  const sourceId = source?.item_id;
  const activityLabel = ACTIVITY_COPY[activity].label;
  const playerActivity = pendingActivity ?? activity;
  const playerActivityLabel = ACTIVITY_COPY[playerActivity].label;
  // A missing health response is deliberately not treated as a failure: the health command can
  // itself be temporarily unavailable. Only an explicit failed subsystem gates its controls.
  const coreAvailable = startupHealth?.core_ready !== false;
  const packsAvailable = startupHealth?.packs_ready !== false;
  const canUseGenreAndFeedback = coreAvailable && packsAvailable;
  const reviewActive = source?.quarantined_review === true && transportActive;
  // Cover art is only present for clean installed-pack sources (never fallback
  // or quarantined review); the backend omits the field otherwise.
  const coverArt =
    source?.cover_art && !source.fallback && !source.quarantined_review ? source.cover_art : null;
  const coverAlt = source ? `${source.item_title} cover art` : "Cover art";

  useEffect(() => {
    const wasActive = previousStatus.current === "playing" || previousStatus.current === "paused";
    previousStatus.current = status;
    if (!wasActive || (status !== "stopped" && status !== "expired")) return;
    void listRecentSessions()
      .then(setRecentSessions)
      .catch(() => undefined);
  }, [status]);

  useEffect(() => {
    if (!transportActive)
      void listRecentSessions()
        .then(setRecentSessions)
        .catch(() => undefined);
  }, [transportActive]);

  useFocusedWindowTransportKeys({
    status,
    pause: session.pause,
    resume: session.resume,
    stop: session.stop,
    navigationAvailable: source?.navigation_available === true && !navigationPending,
    next: async () => {
      await requestNavigation(nextTrack);
    },
    previous: async () => {
      await requestNavigation(previousTrack);
    },
    reportError: session.reportError,
  });

  const requestNavigation = async (command: () => Promise<void>) => {
    if (navigationPending || source?.navigation_available !== true) return;
    const previousItemId = source?.item_id;
    navigationFromItem.current = previousItemId ?? null;
    setNavigationPending(true);
    try {
      await command();
    } catch (error) {
      navigationFromItem.current = null;
      setNavigationPending(false);
      if (navigationTimeout.current) {
        clearTimeout(navigationTimeout.current);
        navigationTimeout.current = null;
      }
      session.reportError(
        `Unable to change track: ${error instanceof Error ? error.message : String(error)}`,
      );
      return;
    }
    // The native command queues the transition and the event bridge publishes
    // the committed identity. Do one immediate fallback read for old shells,
    // but keep the controls guarded until that identity changes.
    void getCurrentSource()
      .then((current) => setSource(current))
      .catch(() => undefined);
    navigationTimeout.current = setTimeout(() => {
      if (navigationFromItem.current !== previousItemId) return;
      navigationFromItem.current = null;
      navigationTimeout.current = null;
      setNavigationPending(false);
      session.reportError("The next track did not become ready. Please try again.");
    }, 10_000);
  };

  const selectActivity = async (next: Activity) => {
    if (activityPending || session.starting) return;
    if (transportActive && activity === next) {
      setPage("home");
      setExpandedPlayer(true);
      return;
    }

    // Show the destination player before stop/reconfigure/decode begins. The native command
    // still prepares only the selected bounded program, but the interaction is immediate.
    setPendingActivity(next);
    setActivityPending(true);
    setPage("home");
    setExpandedPlayer(true);
    resetContentScroll();
    const previousActivity = activity;
    const wasTransportActive = transportActive;
    try {
      if (transportActive) await session.stop();
      const changed = await session.changeActivity(next);
      if (changed === false) throw new Error("The selected focus space could not be loaded.");
      await session.start();
    } catch (error) {
      if (wasTransportActive) {
        try {
          await session.changeActivity(previousActivity);
          await session.start();
        } catch {
          // Preserve the original actionable error; recovery is best effort.
        }
      }
      session.reportError(
        `Unable to start ${ACTIVITY_COPY[next].label}: ${error instanceof Error ? error.message : String(error)}`,
      );
    } finally {
      setActivityPending(false);
      setPendingActivity(null);
    }
  };

  useEffect(() => {
    const transportChanged = previousTransportActive.current !== transportActive;
    previousTransportActive.current = transportActive;
    if (transportChanged && !transportActive && !activityPending) {
      setFocusView(false);
      setExpandedPlayer(false);
    }
  }, [activityPending, transportActive]);

  const exitFocusView = () => {
    setFocusView(false);
    requestAnimationFrame(() => focusEntryControl.current?.focus());
  };

  const resetContentScroll = useCallback(() => {
    requestAnimationFrame(() => {
      const region = scrollRegion.current;
      if (region && typeof region.scrollTo === "function") {
        region.scrollTo({ top: 0, behavior: "auto" });
      } else if (region) {
        region.scrollTop = 0;
      }
      document.documentElement.scrollTop = 0;
      document.body.scrollTop = 0;
    });
  }, []);

  useEffect(() => {
    resetContentScroll();
  }, [expandedPlayer, page, resetContentScroll]);

  useEffect(() => {
    let active = true;
    void getActivityGenres()
      .then((next) => {
        if (active) setGenres(next);
      })
      .catch(() => {
        if (active) setGenres(null);
      });
    return () => {
      active = false;
    };
  }, [activity, catalogueRevision]);

  useEffect(() => {
    let active = true;
    void getActivityMoods()
      .then((next) => {
        if (active) setMoods(next);
      })
      .catch(() => {
        if (active) setMoods(null);
      });
    return () => {
      active = false;
    };
  }, [activity, catalogueRevision, genres?.selected_genre_id]);

  useEffect(() => {
    if (!navigationFromItem.current || !sourceId || sourceId === navigationFromItem.current) return;
    navigationFromItem.current = null;
    setNavigationPending(false);
    if (navigationTimeout.current) {
      clearTimeout(navigationTimeout.current);
      navigationTimeout.current = null;
    }
    void Promise.resolve(resetSessionTimer())
      .then(() => refreshSession())
      .catch((error: unknown) =>
        reportSessionError(
          `Unable to reset the focus timer: ${error instanceof Error ? error.message : String(error)}`,
        ),
      );
  }, [refreshSession, reportSessionError, sourceId]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    let fallbackPoll: ReturnType<typeof setInterval> | null = null;
    const refreshSource = () => {
      void getCurrentSource()
        .then((current) => {
          if (active) setSource(current);
        })
        .catch(() => {
          if (active) setSource(null);
        });
    };
    refreshSource();
    void listenPlaybackChanged((event) => {
      if (!active) return;
      setSource(event.source);
      if (fallbackPoll) {
        clearInterval(fallbackPoll);
        fallbackPoll = null;
      }
    })
      .then((cleanup) => {
        if (!active) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {
        // Older preview shells have no event transport; keep a low-frequency
        // source fallback only for those shells.
        if (active && transportActive) fallbackPoll = setInterval(refreshSource, 500);
      });
    return () => {
      active = false;
      unlisten?.();
      if (fallbackPoll) clearInterval(fallbackPoll);
    };
  }, [status, transportActive]);

  useEffect(
    () => () => {
      if (navigationTimeout.current) clearTimeout(navigationTimeout.current);
    },
    [],
  );

  if (onboardingComplete === null) {
    if (onboardingLoadError) {
      return (
        <>
          {updateNotice}
          <main className="app" aria-labelledby="onboarding-load-title">
            <h1 id="onboarding-load-title">Couldn’t load local preferences</h1>
            <p role="alert">{onboardingLoadError}</p>
            <button type="button" onClick={() => void loadOnboardingPreferences()}>
              Try again
            </button>
          </main>
        </>
      );
    }
    return (
      <>
        {updateNotice}
        <LaunchScreen label="Loading local preferences" />
      </>
    );
  }

  if (!onboardingComplete && !reviewCandidatesLoaded) {
    return (
      <>
        {updateNotice}
        <LaunchScreen label="Loading local review music" />
      </>
    );
  }

  if (!onboardingComplete && reviewCandidates.length === 0) {
    return (
      <>
        {updateNotice}
        <Onboarding
          onComplete={async (intensity, genres) => {
            await completeOnboarding(intensity, genres);
            setOnboardingComplete(true);
            try {
              await session.refresh();
            } catch (error) {
              session.reportError(error instanceof Error ? error.message : String(error));
            }
          }}
        />
      </>
    );
  }

  if (focusView && transportActive && session.snapshot) {
    return (
      <>
        {updateNotice}
        <FocusView
          snapshot={session.snapshot}
          activity={activity}
          activityLabel={activityLabel}
          coverArt={coverArt}
          intensity={session.intensity}
          intensityDisabled={!coreAvailable}
          onChangeIntensity={(value) => void session.changeIntensity(value)}
          onPause={() => void session.pause()}
          onResume={() => void session.resume()}
          onExit={exitFocusView}
        />
      </>
    );
  }

  return (
    <main
      className={`app ${transportActive ? "session-active" : "session-idle"}${page === "home" && expandedPlayer ? " expanded-player" : ""}`}
    >
      <header className="header">
        <div className="header-row">
          <div className="brand-lockup">
            <BrandMark className="brand-mark" />
            <h1>Aria Focus</h1>
          </div>
        </div>
      </header>

      <div
        ref={scrollRegion}
        className={`app-scroll-region page-${page}-scroll-region${page === "home" ? " home-scroll-region" : ""}${page === "home" && expandedPlayer ? " player-scroll-region" : ""}`}
      >
        <ErrorBanner message={session.error} onDismiss={session.dismissError} />
        {updateNotice}

        {page === "home" && transportActive && !expandedPlayer && (
          <section
            className={`mini-player mini-player-${status}`}
            aria-label="Active focus session"
          >
            <button
              type="button"
              className="mini-player-main"
              aria-label="Open player"
              onClick={() => {
                setPage("home");
                setExpandedPlayer(true);
              }}
            >
              <div className="mini-player-info">
                {coverArt && !activityPending ? (
                  <img
                    className="mini-player-cover"
                    src={coverArt}
                    alt={coverAlt}
                    decoding="async"
                  />
                ) : (
                  <ActivityArtwork
                    activity={activity}
                    className="mini-player-cover mini-player-cover--fallback"
                  />
                )}
                <div>
                  <strong>
                    {source?.fallback
                      ? `${activityLabel} preview`
                      : (source?.item_title ?? `${activityLabel} session`)}
                  </strong>
                </div>
              </div>
            </button>
            <div className="mini-player-actions">
              <button
                type="button"
                className="mini-player-toggle"
                disabled={!coreAvailable}
                aria-label={status === "paused" ? "Resume session" : "Pause session"}
                onClick={() => void (status === "paused" ? session.resume() : session.pause())}
              >
                <AppIcon name={status === "paused" ? "play" : "pause"} />
                <span className="visually-hidden">{status === "paused" ? "Resume" : "Pause"}</span>
              </button>
              <button
                type="button"
                className="mini-player-stop"
                onClick={() => void session.stop()}
              >
                <AppIcon name="stop" />
                <span className="visually-hidden">Stop session</span>
              </button>
            </div>
          </section>
        )}

        {page === "home" && (
          <>
            {!expandedPlayer && (
              <>
                <section className="home-choice" aria-label="Choose a focus activity">
                  <div className="home-heading">
                    <h2>Choose your focus space</h2>
                  </div>
                  <ActivitySelector
                    disabled={
                      !coreAvailable ||
                      !packsAvailable ||
                      session.starting ||
                      reviewActive ||
                      activityPending
                    }
                    onSelect={selectActivity}
                  />
                </section>
              </>
            )}

            {expandedPlayer && (
              <section className="player-surface" aria-label="Focus player">
                {coverArt ? (
                  <img
                    className="player-background"
                    src={coverArt}
                    alt=""
                    aria-hidden="true"
                    decoding="async"
                  />
                ) : (
                  <ActivityArtwork
                    className="player-background player-background--fallback"
                    activity={playerActivity}
                  />
                )}
                <div className="player-overlay" aria-hidden="true" />
                <div className="player-content">
                  <div className="player-toolbar">
                    <button
                      type="button"
                      className="back-action player-back-action"
                      onClick={() => setExpandedPlayer(false)}
                    >
                      <AppIcon name="chevron-left" /> Back to Start
                    </button>
                  </div>
                  <p className="eyebrow">
                    {activityPending
                      ? `Loading ${playerActivityLabel}`
                      : transportActive
                        ? `${activityLabel} session`
                        : "Ready when you are"}
                  </p>
                  <SessionTimer snapshot={activityPending ? null : session.snapshot} />

                  {coverArt && !activityPending ? (
                    <img className="player-cover" src={coverArt} alt={coverAlt} decoding="async" />
                  ) : (
                    <ActivityArtwork
                      className="player-cover player-cover--fallback"
                      activity={playerActivity}
                      label={`${playerActivityLabel} artwork`}
                      decorative={false}
                    />
                  )}

                  {source && !activityPending && (
                    <p className="source-label" aria-live="polite">
                      <strong>Audio source:</strong> {source.item_title}
                      {source.fallback
                        ? " · preview tone — no authored music pack is installed"
                        : source.quarantined_review
                          ? " · QUARANTINED local review — provisional transition; not approved/published"
                          : ` · ${source.pack_title}`}
                    </p>
                  )}

                  <TransportControls
                    status={activityPending ? "idle" : status}
                    starting={session.starting || activityPending}
                    activityLabel={playerActivityLabel}
                    startDisabled={
                      activityPending || !coreAvailable || !packsAvailable || reviewActive
                    }
                    actionsDisabled={!coreAvailable || activityPending}
                    onStart={() => void session.start()}
                    onPause={() => void session.pause()}
                    onResume={() => void session.resume()}
                    onStop={() => void session.stop()}
                    navigationAvailable={source?.navigation_available === true}
                    navigationPending={navigationPending}
                    onNext={() => void requestNavigation(nextTrack)}
                    onPrevious={() => void requestNavigation(previousTrack)}
                  />
                  {navigationPending && (
                    <p className="transport-status" role="status" aria-live="polite">
                      Changing track…
                    </p>
                  )}
                  <AdhdModeToggle
                    value={session.intensity}
                    disabled={!coreAvailable}
                    onChange={(value) => void session.changeIntensity(value)}
                  />
                  <MasterVolume
                    variant="compact"
                    value={session.masterVolume}
                    pending={session.volumePending}
                    disabled={!coreAvailable}
                    onChange={session.changeMasterVolume}
                  />
                  {transportActive && (
                    <button
                      ref={focusEntryControl}
                      type="button"
                      className="focus-view-entry"
                      onClick={() => setFocusView(true)}
                    >
                      Enter focus view
                    </button>
                  )}
                </div>
              </section>
            )}
          </>
        )}

        {page === "library" && (
          <section className="library-page" aria-labelledby="library-heading">
            <div className="screen-heading">
              <p className="eyebrow">Library</p>
              <h2 id="library-heading">Your music</h2>
              <p>Generated tracks, favourites, and installed offline packs.</p>
            </div>
            <div className="page-section-tabs library-section-tabs" aria-label="Library sections">
              {(
                [
                  ["overview", "Overview"],
                  ["my_music", "My Music"],
                  ["favorites", "Favourites"],
                  ["packs", "Offline packs"],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  className={librarySection === id ? "selected" : ""}
                  onClick={() => setLibrarySection(id)}
                >
                  {label}
                </button>
              ))}
            </div>
            <div className="library-section-content">
              {(librarySection === "overview" || librarySection === "my_music") && (
                <MyMusicLibrary
                  disabled={!coreAvailable || !packsAvailable || transportActive}
                  onError={session.reportError}
                  onStarted={async () => {
                    await session.adoptStartedSession();
                    setPage("home");
                  }}
                  onCatalogueChange={() => {
                    setCatalogueRevision((revision) => revision + 1);
                    setContentPacksRevision((revision) => revision + 1);
                  }}
                />
              )}
              {(librarySection === "overview" || librarySection === "favorites") && (
                <FavoritesLibrary
                  active={transportActive}
                  disabled={!coreAvailable || !packsAvailable || session.starting}
                  revision={favoritesRevision}
                  onStarted={async () => {
                    await session.refresh();
                    setPage("home");
                  }}
                  onError={session.reportError}
                />
              )}
              {(librarySection === "overview" || librarySection === "packs") && (
                <ContentPacks
                  key={contentPacksRevision}
                  disabled={!packsAvailable}
                  onCatalogueChange={() => setCatalogueRevision((revision) => revision + 1)}
                />
              )}
              {(librarySection === "overview" || librarySection === "my_music") && (
                <StudioLibraryCard onOpen={() => setPage("studio")} />
              )}
            </div>
          </section>
        )}

        {page === "history" && <RecentSessions sessions={recentSessions} />}

        {page === "studio" && (
          <CloudGenerationPanel view="create" onOpenSettings={() => setPage("settings")} />
        )}

        {page === "settings" && (
          <section className="settings-page" aria-labelledby="settings-heading">
            <div className="screen-heading">
              <p className="eyebrow">Settings</p>
              <h2 id="settings-heading">Make it comfortable</h2>
              <p>One section at a time keeps this page clear and usable in a small window.</p>
            </div>
            {(startupHealth && !startupHealth.core_ready) ||
            (startupHealth && !startupHealth.packs_ready) ? (
              <StartupRecovery
                health={startupHealth}
                busy={retryingStartup}
                retryError={startupRetryError}
                onRetry={() => void retryStartupServices()}
              />
            ) : null}
            <div className="page-section-tabs" role="tablist" aria-label="Settings sections">
              {(
                [
                  ["sound", "Sound & timer"],
                  ["focus", "Focus controls"],
                  ["connection", "Music creation"],
                  ["help", "Help & about"],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  id={`settings-tab-${id}`}
                  role="tab"
                  aria-selected={settingsSection === id}
                  aria-controls={`settings-panel-${id}`}
                  className={settingsSection === id ? "selected" : ""}
                  onClick={() => setSettingsSection(id)}
                >
                  {label}
                </button>
              ))}
            </div>
            {reviewCandidates.length > 0 && (
              <button type="button" className="settings-row" onClick={() => setPage("review")}>
                <AppIcon name="sliders" />
                <span>
                  <strong>Review local music</strong>
                  <small>Blind candidate review</small>
                </span>
                <span aria-hidden="true">›</span>
              </button>
            )}
            {settingsSection === "connection" && (
              <div
                id="settings-panel-connection"
                role="tabpanel"
                aria-labelledby="settings-tab-connection"
              >
                <CloudGenerationPanel view="settings" onOpenCreate={() => setPage("studio")} />
              </div>
            )}
            {!coreAvailable && settingsSection === "sound" && (
              <div
                id="settings-panel-sound-availability"
                role="tabpanel"
                aria-labelledby="settings-tab-sound"
                className="settings-section-content"
              >
                <IntensitySelector
                  value={session.intensity}
                  disabled
                  onChange={(i) => void session.changeIntensity(i)}
                />
                <MasterVolume
                  value={session.masterVolume}
                  pending={session.volumePending}
                  disabled
                  onChange={session.changeMasterVolume}
                />
              </div>
            )}
            {settingsSection === "focus" && (
              <div
                id="settings-panel-focus"
                role="tabpanel"
                aria-labelledby="settings-tab-focus"
                className="settings-section-content"
              >
                <IntensitySelector
                  value={session.intensity}
                  disabled={!coreAvailable}
                  onChange={(i) => void session.changeIntensity(i)}
                />
                <MasterVolume
                  value={session.masterVolume}
                  pending={session.volumePending}
                  disabled={!coreAvailable}
                  onChange={session.changeMasterVolume}
                />
              </div>
            )}
            {settingsSection === "sound" && (
              <section
                id="settings-panel-sound"
                role="tabpanel"
                aria-labelledby="settings-tab-sound"
                className="settings-section-content settings-session-options"
                aria-label="Sound and timer options"
              >
                <h2>Sound and timer</h2>
                <GenreSelector
                  state={genres}
                  disabled={!canUseGenreAndFeedback || session.starting || reviewActive}
                  onChange={(genreId) =>
                    void setActivityGenre(genreId)
                      .then(setGenres)
                      .catch((error: unknown) =>
                        session.reportError(
                          `Unable to change music genre: ${error instanceof Error ? error.message : String(error)}`,
                        ),
                      )
                  }
                />
                <MoodSelector
                  state={moods}
                  disabled={!canUseGenreAndFeedback || session.starting || reviewActive}
                  onChange={(moodId) =>
                    void setActivityMood(moodId)
                      .then(setMoods)
                      .catch((error: unknown) =>
                        session.reportError(
                          `Unable to change music mood: ${error instanceof Error ? error.message : String(error)}`,
                        ),
                      )
                  }
                />
                <SessionTypeSelector
                  value={session.snapshot?.kind ?? { kind: "infinite" }}
                  disabled={!coreAvailable || session.starting || reviewActive}
                  onChange={(kind) => void session.changeSessionType(kind)}
                />
              </section>
            )}
            {settingsSection === "help" && (
              <div
                id="settings-panel-help"
                role="tabpanel"
                aria-labelledby="settings-tab-help"
                className="settings-section-content"
              >
                <AboutAriaFocus />
                <Disclaimer />
                {provenance && source?.fallback && (
                  <details className="provenance">
                    <summary>Test tone details</summary>
                    <p>{provenance.notes}</p>
                  </details>
                )}
              </div>
            )}
          </section>
        )}

        {page === "review" && (
          <section className="review-page" aria-label="Local music review">
            <div className="screen-heading">
              <button type="button" className="back-action" onClick={() => setPage("settings")}>
                <AppIcon name="chevron-left" /> Settings
              </button>
              <p className="eyebrow">Local review</p>
              <h2>Candidate music</h2>
            </div>
            <QuarantinedReview
              candidates={reviewCandidates}
              active={transportActive}
              disabled={!coreAvailable || session.starting}
              onStart={(id) =>
                void startReviewCandidate(id)
                  .then(async () => {
                    await session.refresh();
                    setPage("home");
                  })
                  .catch((error: unknown) =>
                    session.reportError(
                      `Unable to start quarantined review: ${error instanceof Error ? error.message : String(error)}`,
                    ),
                  )
              }
            />
          </section>
        )}

        <footer className="footer">
          <span>Offline focus music · Focus / {activityLabel}</span>
        </footer>
      </div>

      <nav className="app-navigation" aria-label="Main navigation">
        {(
          [
            ["home", "Home", "home"],
            ["library", "Library", "library"],
            ["studio", "Create", "create"],
            ["history", "History", "history"],
            ["settings", "Settings", "settings"],
          ] as const
        ).map(([id, label, icon]) => (
          <button
            key={id}
            type="button"
            className={page === id ? "selected" : ""}
            aria-current={page === id ? "page" : undefined}
            onClick={() => {
              setPage(id);
              setExpandedPlayer(id === "home" && transportActive);
              resetContentScroll();
            }}
          >
            <AppIcon name={icon} />
            {label}
          </button>
        ))}
      </nav>
    </main>
  );
}
