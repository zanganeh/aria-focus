import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

/**
 * The updater is deliberately best-effort. A missing release, placeholder key,
 * or unavailable network must never affect audio, startup recovery, or onboarding.
 */
export async function findAvailableUpdate(): Promise<Update | null> {
  try {
    return await check();
  } catch {
    return null;
  }
}

export async function installAndRelaunch(update: Update): Promise<void> {
  await update.downloadAndInstall();
  await relaunch();
}
