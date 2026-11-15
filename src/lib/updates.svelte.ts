// App updates from the GitHub releases. The Tauri updater reads
// `latest.json` there (`plugins.updater` in tauri.conf.json), checks the
// installer's signature against the public key and runs it. The installer
// ends this process, so every window saves its notes first.

import { getVersion } from "@tauri-apps/api/app";
import { emit, listen } from "@tauri-apps/api/event";
import { getAllWindows } from "@tauri-apps/api/window";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

const FLUSH = "rusidian://update-flush";
const FLUSHED = "rusidian://update-flushed";

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

type Status = "idle" | "checking" | "latest" | "available" | "installing" | "error";

class Updates {
  /** The running version. */
  version = $state("");
  status = $state<Status>("idle");
  update = $state.raw<Update | null>(null);
  /** Share downloaded while installing, 0..1 (null while the size is unknown). */
  progress = $state<number | null>(null);
  error = $state<string | null>(null);

  async loadVersion() {
    if (!this.version) this.version = await getVersion().catch(() => "");
  }

  /** Ask GitHub for a newer version. */
  async check(): Promise<Update | null> {
    if (this.status === "checking" || this.status === "installing") return this.update;
    this.status = "checking";
    this.error = null;
    try {
      const u = await check();
      this.update = u;
      this.status = u ? "available" : "latest";
      return u;
    } catch (e) {
      this.status = "error";
      this.error = String(e);
      return null;
    }
  }

  /** Download, save every window's notes, install and restart. */
  async install() {
    const u = this.update;
    if (!u || this.status === "installing") return;
    this.status = "installing";
    this.progress = null;
    this.error = null;
    try {
      let total = 0;
      let done = 0;
      await u.download((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? 0;
        else if (ev.event === "Progress") {
          done += ev.data.chunkLength;
          this.progress = total ? Math.min(1, done / total) : null;
        }
      });
      // Saved after the download, so what was typed meanwhile is kept too.
      await flushEveryWindow();
      await u.install();
      await relaunch();
    } catch (e) {
      this.status = "error";
      this.error = String(e);
    }
  }
}

export const updates = new Updates();

/** Every window saves (see `onUpdateFlush`); bounded, so an update never hangs on it. */
async function flushEveryWindow() {
  const count = (await getAllWindows()).length;
  let acked = 0;
  let allDone!: () => void;
  const all = new Promise<void>((r) => (allDone = r));
  const unlisten = await listen(FLUSHED, () => {
    if (++acked >= count) allDone();
  });
  try {
    await emit(FLUSH);
    await Promise.race([all, sleep(5000)]);
  } finally {
    unlisten();
  }
}

/** In each window: save its notes when an update is about to install. */
export function onUpdateFlush(flush: () => Promise<void>): Promise<() => void> {
  return listen(FLUSH, async () => {
    try {
      await Promise.race([flush(), sleep(4000)]);
    } catch {
      // Answer anyway: the installer will not wait.
    }
    await emit(FLUSHED);
  });
}
