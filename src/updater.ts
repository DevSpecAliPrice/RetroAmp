import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask, message } from "@tauri-apps/plugin-dialog";

export async function checkForUpdates(opts: { silent?: boolean } = {}) {
  const { silent = true } = opts;

  let update;
  try {
    update = await check();
  } catch (e) {
    console.warn("[updater] check failed:", e);
    if (!silent) {
      await message(`Could not check for updates: ${e}`, {
        title: "RetroAmp",
        kind: "warning",
      });
    }
    return;
  }

  if (!update) {
    if (!silent) {
      await message("You're on the latest version.", {
        title: "RetroAmp",
        kind: "info",
      });
    }
    return;
  }

  const yes = await ask(
    `RetroAmp ${update.version} is available. Install now?\n\n` +
      `You're on ${update.currentVersion}.` +
      (update.body ? `\n\nRelease notes:\n${update.body}` : ""),
    { title: "RetroAmp update available", kind: "info" },
  );
  if (!yes) return;

  try {
    await update.downloadAndInstall();
  } catch (e) {
    console.error("[updater] install failed:", e);
    await message(`Update failed: ${e}`, {
      title: "RetroAmp",
      kind: "error",
    });
    return;
  }

  await relaunch();
}
