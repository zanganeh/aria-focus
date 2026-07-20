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
import { StudioPage } from "./components/StudioPage";
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
type HomeScreen = "choose" | "sound" | "timer";

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
  const [homeScreen, setHomeScreen] = useState<HomeScreen>("choose");
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
  const activity = session.snapshot?.activity ?? "deep_work";
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
    setNavigationPending(true);
    try {
      await command();
    } catch (error) {
      setNavigationPending(false);
      session.reportError(
        `Unable to change track: ${error instanceof Error ? error.message : String(error)}`,
      );
      return;
    }

    // The native command only queues a callback-safe transition. Keep the player responsive
    // while the audio callback crossfades and publishes the new identity in the background.
    void (async () => {
      let current = await getCurrentSource();
      const deadline = Date.now() + 20_000;
      while (current.item_id === previousItemId && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 100));
        current = await getCurrentSource();
      }
      setSource(current);
      if (current.item_id !== previousItemId) {
        await resetSessionTimer();
        await session.refresh();
      }
      setNavigationPending(false);
    })().catch((error: unknown) => {
      setNavigationPending(false);
      session.reportError(
        `Unable to finish changing track: ${error instanceof Error ? error.message : String(error)}`,
      );
    });
  };

  const selectActivity = async (next: Activity) => {
    if (activityPending || session.starting) return;
    if (transportActive && activity === next) {
      setPage("home");
      setHomeScreen("choose");
      setExpandedPlayer(true);
      return;
    }

    // Show the destination player before stop/reconfigure/decode begins. The native command
    // still prepares only the selected bounded program, but the interaction is immediate.
    setPendingActivity(next);
    setActivityPending(true);
    setPage("home");
    setHomeScreen("choose");
    setExpandedPlayer(true);
    resetContentScroll();
    try {
      if (transportActive) await session.stop();
      const changed = await session.changeActivity(next);
      if (changed === false) return;
      await session.start();
    } catch (error) {
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
  }, [expandedPlayer, homeScreen, page, resetContentScroll]);

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
    let active = true;
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
    const poll = transportActive ? setInterval(refreshSource, 500) : null;
    return () => {
      active = false;
      if (poll) clearInterval(poll);
    };
  }, [status, transportActive]);

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

        {transportActive && !expandedPlayer && (
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
                setHomeScreen("choose");
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
                {!transportActive && homeScreen === "sound" && (
                  <section className="setup-flow guided-setup" aria-label="Choose sound">
                    <div className="screen-heading">
                      <button
                        type="button"
                        className="back-action"
                        onClick={() => setHomeScreen("choose")}
                      >
                        <AppIcon name="chevron-left" /> Back
                      </button>
                      <p className="eyebrow">Sound</p>
                      <h2>Make it feel right</h2>
                      <p>These choices filter the local music for {activityLabel}.</p>
                    </div>
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
                              `Unable to change mood: ${error instanceof Error ? error.message : String(error)}`,
                            ),
                          )
                      }
                    />
                    <button
                      type="button"
                      className="primary setup-next"
                      onClick={() => setHomeScreen("timer")}
                    >
                      Choose timer
                    </button>
                  </section>
                )}

                {!transportActive && homeScreen === "timer" && (
                  <section className="setup-flow guided-setup" aria-label="Choose session timer">
                    <div className="screen-heading">
                      <button
                        type="button"
                        className="back-action"
                        onClick={() => setHomeScreen("sound")}
                      >
                        <AppIcon name="chevron-left" /> Back
                      </button>
                      <p className="eyebrow">Time</p>
                      <h2>How long?</h2>
                      <p>Keep the default if you just want to begin.</p>
                    </div>
                    <SessionTypeSelector
                      value={session.snapshot?.kind ?? { kind: "infinite" }}
                      disabled={!coreAvailable || session.starting || reviewActive}
                      onChange={(kind) => void session.changeSessionType(kind)}
                    />
                    <button
                      type="button"
                      className="primary setup-next"
                      disabled={
                        !coreAvailable || !packsAvailable || reviewActive || session.starting
                      }
                      onClick={() => void session.start()}
                    >
                      {session.starting ? "Starting…" : `Start ${activityLabel}`}
                    </button>
                  </section>
                )}
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

        {page === "library" && <StudioLibraryCard onOpen={() => setPage("studio")} />}
        {page === "library" && (
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
        {page === "library" && (
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
        {page === "library" && (
          <ContentPacks
            key={contentPacksRevision}
            disabled={!packsAvailable}
            onCatalogueChange={() => setCatalogueRevision((revision) => revision + 1)}
          />
        )}

        {page === "history" && <RecentSessions sessions={recentSessions} />}

        {page === "studio" && <StudioPage onReturn={() => setPage("library")} />}

        {page === "settings" && startupHealth && (
          <StartupRecovery
            health={startupHealth}
            busy={retryingStartup}
            retryError={startupRetryError}
            onRetry={() => void retryStartupServices()}
          />
        )}
        {page === "settings" && (
          <>
            <section className="settings-menu" aria-labelledby="settings-heading">
              <div className="screen-heading">
                <p className="eyebrow">Settings</p>
                <h2 id="settings-heading">Make it comfortable</h2>
                <p>These stay on this device.</p>
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
            </section>

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

            <details className="settings-collapsible">
              <summary>
                <strong>Sound and timer</strong>
                <small>Genre, mood, and session timing</small>
              </summary>
              <h2 className="visually-hidden">Sound and timer</h2>
              <section className="settings-session-options" aria-label="Sound and timer options">
                <details className="settings-option-collapsible">
                  <summary>
                    <strong>Music genre</strong>
                    <small>Choose the sound style</small>
                  </summary>
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
                </details>
                <details className="settings-option-collapsible">
                  <summary>
                    <strong>Mood</strong>
                    <small>Choose the emotional direction</small>
                  </summary>
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
                </details>
                <details className="settings-option-collapsible">
                  <summary>
                    <strong>Session timer</strong>
                    <small>Infinite, countdown, or interval</small>
                  </summary>
                  <SessionTypeSelector
                    value={session.snapshot?.kind ?? { kind: "infinite" }}
                    disabled={!coreAvailable || session.starting || reviewActive}
                    onChange={(kind) => void session.changeSessionType(kind)}
                  />
                </details>
              </section>
            </details>

            {provenance && source?.fallback && (
              <details className="provenance">
                <summary>Test tone provenance &amp; licence</summary>
                <dl>
                  <dt>Asset</dt>
                  <dd>{provenance.title}</dd>
                  <dt>Generator</dt>
                  <dd>
                    {provenance.generator} v{provenance.generator_version}
                  </dd>
                  <dt>Source</dt>
                  <dd>{provenance.source}</dd>
                  <dt>Licence</dt>
                  <dd>{provenance.licence}</dd>
                  <dt>Voice / lyrics</dt>
                  <dd>
                    {provenance.contains_voice_or_speech ? "yes" : "no"} /{" "}
                    {provenance.contains_lyrics ? "yes" : "no"}
                  </dd>
                  <dt>Looping</dt>
                  <dd>
                    {provenance.loops_seamlessly ? "seamless" : "crossfaded"},{" "}
                    {provenance.duration_seconds}s @ {provenance.sample_rate_hz}Hz
                  </dd>
                </dl>
                <p className="provenance-notes">{provenance.notes}</p>
              </details>
            )}

            <details className="settings-collapsible settings-about-collapsible">
              <summary>
                <strong>About &amp; help</strong>
                <small>Version, feedback links, and safety note</small>
              </summary>
              <div className="settings-collapsible-content">
                <AboutAriaFocus />
                <Disclaimer />
              </div>
            </details>
          </>
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
              setHomeScreen("choose");
              setExpandedPlayer(false);
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
