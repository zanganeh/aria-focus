import { useCallback, useEffect, useRef, useState } from "react";
import { ActivitySelector } from "./components/ActivitySelector";
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
  completeOnboarding,
  getOnboardingPreferences,
  listRecentSessions,
} from "./lib/api";
import { ACTIVITY_COPY } from "./lib/activities";
import { useSession } from "./hooks/useSession";
import { useFocusedWindowTransportKeys } from "./hooks/useFocusedWindowTransportKeys";
import type {
  ActivityGenreState,
  ActivityMoodState,
  CurrentSource,
  Provenance,
  StartupHealth,
  ReviewCandidate,
  SessionHistoryRecord,
} from "./lib/types";

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
  const [page, setPage] = useState<AppPage>("home");
  const [homeScreen, setHomeScreen] = useState<HomeScreen>("choose");
  const [navigationPending, setNavigationPending] = useState(false);
  const [onboardingComplete, setOnboardingComplete] = useState<boolean | null>(null);
  const [onboardingLoadError, setOnboardingLoadError] = useState<string | null>(null);
  const [recentSessions, setRecentSessions] = useState<SessionHistoryRecord[]>([]);
  const previousStatus = useRef<string | null>(null);
  const focusEntryControl = useRef<HTMLButtonElement>(null);
  const retryInFlight = useRef(false);
  const healthRequest = useRef(0);
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
    setNavigationPending(true);
    try {
      await command();
      // The label remains renderer-owned; polling observes only the committed atomic track.
      const current = await getCurrentSource();
      setSource(current);
    } catch (error) {
      session.reportError(
        `Unable to change track: ${error instanceof Error ? error.message : String(error)}`,
      );
    } finally {
      setNavigationPending(false);
    }
  };

  useEffect(() => {
    if (!transportActive) setFocusView(false);
  }, [transportActive]);

  const exitFocusView = () => {
    setFocusView(false);
    requestAnimationFrame(() => focusEntryControl.current?.focus());
  };

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
        <main className="app" aria-labelledby="onboarding-load-title">
          <h1 id="onboarding-load-title">Couldn’t load local preferences</h1>
          <p role="alert">{onboardingLoadError}</p>
          <button type="button" onClick={() => void loadOnboardingPreferences()}>
            Try again
          </button>
        </main>
      );
    }
    return <LaunchScreen label="Loading local preferences" />;
  }

  if (!onboardingComplete && !reviewCandidatesLoaded) {
    return <LaunchScreen label="Loading local review music" />;
  }

  if (!onboardingComplete && reviewCandidates.length === 0) {
    return (
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
    );
  }

  if (focusView && transportActive && session.snapshot) {
    return (
      <FocusView
        snapshot={session.snapshot}
        activityLabel={activityLabel}
        coverArt={coverArt}
        onPause={() => void session.pause()}
        onResume={() => void session.resume()}
        onExit={exitFocusView}
      />
    );
  }

  return (
    <main className={`app ${transportActive ? "session-active" : "session-idle"}`}>
      <header className="header">
        <div className="header-row">
          <div className="brand-lockup">
            <BrandMark className="brand-mark" />
            <h1>Aria Focus</h1>
          </div>
        </div>
      </header>

      <div className="app-scroll-region">
        <ErrorBanner message={session.error} onDismiss={session.dismissError} />

        {page !== "home" && transportActive && (
          <section className="mini-player" aria-label="Active focus session">
            <div className="mini-player-info">
              {coverArt ? (
                <img className="mini-player-cover" src={coverArt} alt={coverAlt} decoding="async" />
              ) : null}
              <div>
                <strong>{source?.item_title ?? `${activityLabel} session`}</strong>
                <span>{status === "paused" ? "Paused" : "Playing"}</span>
              </div>
            </div>
            <div className="mini-player-actions">
              <button
                type="button"
                className="mini-player-toggle"
                disabled={!coreAvailable}
                aria-label={status === "paused" ? "Resume session" : "Pause session"}
                onClick={() => void (status === "paused" ? session.resume() : session.pause())}
              >
                {status === "paused" ? "Resume" : "Pause"}
              </button>
              <button type="button" onClick={() => setPage("home")}>
                Open player
              </button>
            </div>
          </section>
        )}

        {page === "home" && (
          <>
            {!transportActive && (
              <section className="home-choice" aria-label="Choose a focus activity">
                <ActivitySelector
                  disabled={!coreAvailable || !packsAvailable || session.starting || reviewActive}
                  onSelect={async (next) => {
                    await session.changeActivity(next);
                    await session.start();
                    document.documentElement.scrollTop = 0;
                    document.body.scrollTop = 0;
                  }}
                />
              </section>
            )}

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
                  disabled={!coreAvailable || !packsAvailable || reviewActive || session.starting}
                  onClick={() => void session.start()}
                >
                  {session.starting ? "Starting…" : `Start ${activityLabel}`}
                </button>
              </section>
            )}

            {transportActive && (
              <section className="player-surface" aria-label="Focus player">
                {coverArt && (
                  <img
                    className="player-background"
                    src={coverArt}
                    alt=""
                    aria-hidden="true"
                    decoding="async"
                  />
                )}
                <div className="player-overlay" aria-hidden="true" />
                <div className="player-content">
                  <p className="eyebrow">
                    {transportActive ? `${activityLabel} session` : "Ready when you are"}
                  </p>
                  <SessionTimer snapshot={session.snapshot} />

                  {coverArt ? (
                    <img className="player-cover" src={coverArt} alt={coverAlt} decoding="async" />
                  ) : (
                    <div className="player-cover player-cover--none" aria-hidden="true" />
                  )}

                  {source && (
                    <p className="source-label" aria-live="polite">
                      <strong>Audio source:</strong> {source.item_title}
                      {source.fallback
                        ? " · explicit procedural fallback"
                        : source.quarantined_review
                          ? " · QUARANTINED local review — provisional transition; not approved/published"
                          : ` · ${source.pack_title}`}
                    </p>
                  )}

                  <TransportControls
                    status={status}
                    starting={session.starting}
                    activityLabel={activityLabel}
                    startDisabled={!coreAvailable || !packsAvailable || reviewActive}
                    actionsDisabled={!coreAvailable}
                    onStart={() => void session.start()}
                    onPause={() => void session.pause()}
                    onResume={() => void session.resume()}
                    onStop={() => void session.stop()}
                    navigationAvailable={source?.navigation_available === true}
                    navigationPending={navigationPending}
                    onNext={() => void requestNavigation(nextTrack)}
                    onPrevious={() => void requestNavigation(previousTrack)}
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
        )}

        {page === "settings" && (
          <IntensitySelector
            value={session.intensity}
            disabled={!coreAvailable}
            onChange={(i) => void session.changeIntensity(i)}
          />
        )}
        {page === "settings" && (
          <MasterVolume
            value={session.masterVolume}
            pending={session.volumePending}
            disabled={!coreAvailable}
            onChange={session.changeMasterVolume}
          />
        )}
        {page === "settings" && (
          <section className="settings-session-options" aria-labelledby="session-options-heading">
            <div className="screen-heading">
              <p className="eyebrow">Optional</p>
              <h2 id="session-options-heading">Sound and timer</h2>
              <p>Leave these alone to use the app defaults.</p>
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

        {page === "settings" && provenance && source?.fallback && (
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

        {page === "settings" && <AboutAriaFocus />}

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

        {page === "settings" && <Disclaimer />}

        <footer className="footer">
          <span>Offline focus music · Focus / {activityLabel}</span>
          {transportActive && <span aria-hidden="true"> · playing</span>}
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
