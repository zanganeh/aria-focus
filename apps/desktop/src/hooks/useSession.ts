import { useCallback, useEffect, useRef, useState } from "react";
import {
  getSnapshot,
  pauseSession,
  resumeSession,
  setActivity as apiSetActivity,
  setIntensity as apiSetIntensity,
  getMasterVolume,
  setMasterVolume as apiSetMasterVolume,
  setSessionType as apiSetSessionType,
  startSession,
  stopSession,
} from "../lib/api";
import type {
  Activity,
  Intensity,
  SessionSnapshot,
  SessionStatus,
  SessionType,
} from "../lib/types";

const POLL_MS = 250;

export interface SessionController {
  snapshot: SessionSnapshot | null;
  intensity: Intensity;
  masterVolume: number;
  volumePending: boolean;
  starting: boolean;
  error: string | null;
  start: () => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  stop: () => Promise<void>;
  changeActivity: (activity: Activity) => Promise<boolean>;
  changeIntensity: (i: Intensity) => Promise<void>;
  changeMasterVolume: (volume: number) => void;
  changeSessionType: (kind: SessionType) => Promise<void>;
  dismissError: () => void;
  reportError: (message: string) => void;
  refresh: () => Promise<void>;
  adoptStartedSession: () => Promise<void>;
  clearSessionLoadError: () => void;
}

function errorDetail(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "An unknown native command error occurred.";
}

export function useSession(): SessionController {
  const [snapshot, setSnapshot] = useState<SessionSnapshot | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<{ message: string; sessionLoad: boolean } | null>(null);
  const intensityRef = useRef<Intensity>("medium");
  const confirmedVolume = useRef(70);
  const requestedVolume = useRef<number | null>(null);
  const volumeSaving = useRef(false);
  const [masterVolume, setMasterVolume] = useState(70);
  const [volumePending, setVolumePending] = useState(false);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const refresh = useCallback(async () => {
    try {
      const snap = await getSnapshot();
      setSnapshot(snap);
      intensityRef.current = snap.intensity;
    } catch (e) {
      setError({
        message: `Unable to load the local session: ${errorDetail(e)}`,
        sessionLoad: true,
      });
    }
  }, []);

  const startPolling = useCallback(() => {
    if (pollRef.current) return;
    pollRef.current = setInterval(() => {
      void refresh();
    }, POLL_MS);
  }, [refresh]);

  const stopPolling = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  useEffect(() => {
    void refresh();
    void getMasterVolume()
      .then((volume) => {
        confirmedVolume.current = volume;
        setMasterVolume(volume);
      })
      .catch((e) =>
        setError({ message: `Unable to load master volume: ${errorDetail(e)}`, sessionLoad: true }),
      );
    return () => stopPolling();
  }, [refresh, stopPolling]);

  const start = useCallback(async () => {
    setStarting(true);
    try {
      await startSession();
      await refresh();
      startPolling();
    } catch (e) {
      setError({ message: `Unable to start the session: ${errorDetail(e)}`, sessionLoad: false });
    } finally {
      setStarting(false);
    }
  }, [refresh, startPolling]);

  const adoptStartedSession = useCallback(async () => {
    await refresh();
    startPolling();
  }, [refresh, startPolling]);

  const pause = useCallback(async () => {
    try {
      await pauseSession();
      stopPolling();
      await refresh();
    } catch (e) {
      setError({ message: `Unable to pause the session: ${errorDetail(e)}`, sessionLoad: false });
    }
  }, [refresh, stopPolling]);

  const resume = useCallback(async () => {
    try {
      await resumeSession();
      await refresh();
      startPolling();
    } catch (e) {
      setError({ message: `Unable to resume the session: ${errorDetail(e)}`, sessionLoad: false });
    }
  }, [refresh, startPolling]);

  const stop = useCallback(async () => {
    try {
      await stopSession();
      stopPolling();
      await refresh();
    } catch (e) {
      setError({ message: `Unable to stop the session: ${errorDetail(e)}`, sessionLoad: false });
    }
  }, [refresh, stopPolling]);

  const changeActivity = useCallback(
    async (activity: Activity) => {
      try {
        await apiSetActivity(activity);
        await refresh();
        return true;
      } catch (e) {
        setError({ message: `Unable to change activity: ${errorDetail(e)}`, sessionLoad: false });
        return false;
      }
    },
    [refresh],
  );

  const changeIntensity = useCallback(
    async (i: Intensity) => {
      try {
        await apiSetIntensity(i);
        intensityRef.current = i;
        await refresh();
      } catch (e) {
        setError({
          message: `Unable to change stimulation intensity: ${errorDetail(e)}`,
          sessionLoad: false,
        });
      }
    },
    [refresh],
  );

  const changeMasterVolume = useCallback((volume: number) => {
    if (!Number.isInteger(volume) || volume < 0 || volume > 100) return;
    requestedVolume.current = volume;
    setMasterVolume(volume);
    setVolumePending(true);
    if (volumeSaving.current) return;
    volumeSaving.current = true;
    void (async () => {
      while (requestedVolume.current !== null) {
        const next = requestedVolume.current;
        requestedVolume.current = null;
        try {
          const confirmed = await apiSetMasterVolume(next);
          confirmedVolume.current = confirmed;
          setMasterVolume(confirmed);
        } catch (e) {
          requestedVolume.current = null;
          setMasterVolume(confirmedVolume.current);
          setError({
            message: `Unable to save master volume: ${errorDetail(e)}`,
            sessionLoad: false,
          });
        }
      }
      volumeSaving.current = false;
      setVolumePending(false);
    })();
  }, []);

  const changeSessionType = useCallback(
    async (kind: SessionType) => {
      try {
        await apiSetSessionType(kind);
        await refresh();
      } catch (e) {
        setError({
          message: `Unable to change session timer: ${errorDetail(e)}`,
          sessionLoad: false,
        });
      }
    },
    [refresh],
  );

  const status: SessionStatus = snapshot?.status ?? "idle";
  useEffect(() => {
    if (status === "expired") {
      stopPolling();
    }
  }, [status, stopPolling]);

  return {
    snapshot,
    intensity: snapshot?.intensity ?? intensityRef.current,
    masterVolume,
    volumePending,
    starting,
    error: error?.message ?? null,
    start,
    pause,
    resume,
    stop,
    changeActivity,
    changeIntensity,
    changeMasterVolume,
    changeSessionType,
    dismissError: () => setError(null),
    reportError: (message) => setError({ message, sessionLoad: false }),
    refresh,
    adoptStartedSession,
    clearSessionLoadError: () => setError((current) => (current?.sessionLoad ? null : current)),
  };
}
