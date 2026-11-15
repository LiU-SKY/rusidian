// App preferences, kept in localStorage (per machine, not per vault).
// app.html applies the saved theme before the first paint; this store
// takes over once the app is running.

export type Theme = "system" | "light" | "dark";

const KEY = "rusidian.settings";
export const FONT_MIN = 12;
export const FONT_MAX = 24;

interface Saved {
  theme: Theme;
  fontSize: number;
  readableWidth: boolean;
  /** Editing hides markup on other lines (false: plain source mode). */
  livePreview: boolean;
  /** System notifications for task reminders and timed tasks. */
  notifications: boolean;
  /** The note's name (editable) and its dates above the text. */
  inlineTitle: boolean;
}

const DEFAULTS: Saved = { theme: "system", fontSize: 16, readableWidth: true, livePreview: true, notifications: true, inlineTitle: true };

function load(): Saved {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return {
      theme: ["system", "light", "dark"].includes(raw.theme) ? raw.theme : DEFAULTS.theme,
      fontSize: Number.isFinite(raw.fontSize)
        ? Math.min(FONT_MAX, Math.max(FONT_MIN, Math.round(raw.fontSize)))
        : DEFAULTS.fontSize,
      readableWidth: typeof raw.readableWidth === "boolean" ? raw.readableWidth : DEFAULTS.readableWidth,
      livePreview: typeof raw.livePreview === "boolean" ? raw.livePreview : DEFAULTS.livePreview,
      notifications: typeof raw.notifications === "boolean" ? raw.notifications : DEFAULTS.notifications,
      inlineTitle: typeof raw.inlineTitle === "boolean" ? raw.inlineTitle : DEFAULTS.inlineTitle,
    };
  } catch {
    return { ...DEFAULTS };
  }
}

class Settings {
  theme = $state<Theme>(DEFAULTS.theme);
  fontSize = $state(DEFAULTS.fontSize);
  readableWidth = $state(DEFAULTS.readableWidth);
  livePreview = $state(DEFAULTS.livePreview);
  notifications = $state(DEFAULTS.notifications);
  inlineTitle = $state(DEFAULTS.inlineTitle);
  /** The OS is in dark mode (used when theme is "system"). */
  systemDark = $state(false);
  /** The theme actually shown. */
  resolved = $derived<"light" | "dark">(
    this.theme === "system" ? (this.systemDark ? "dark" : "light") : this.theme,
  );

  constructor() {
    if (typeof window === "undefined") return;
    Object.assign(this, load());
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    this.systemDark = media.matches;
    media.addEventListener("change", (e) => (this.systemDark = e.matches));
    // Another window changed the settings: follow it (theme, font size, …).
    window.addEventListener("storage", (e) => {
      if (e.key === KEY) Object.assign(this, load());
    });
  }

  /** The settings as plain data (also what sync carries to other devices). */
  snapshot(): Saved {
    return {
      theme: this.theme,
      fontSize: this.fontSize,
      readableWidth: this.readableWidth,
      livePreview: this.livePreview,
      notifications: this.notifications,
      inlineTitle: this.inlineTitle,
    };
  }

  /** Take settings that came from another device (unknown keys ignored). */
  applySnapshot(data: unknown) {
    try {
      localStorage.setItem(KEY, JSON.stringify(data));
    } catch {
      return;
    }
    Object.assign(this, load());
  }

  save() {
    try {
      localStorage.setItem(KEY, JSON.stringify(this.snapshot()));
    } catch {
      // Storage unavailable: the choice lasts until the app closes.
    }
  }

  /** Push the preferences into the document (theme attribute, CSS variables). */
  apply() {
    const root = document.documentElement;
    root.dataset.theme = this.resolved;
    root.style.setProperty("--editor-font-size", `${this.fontSize}px`);
    root.style.setProperty("--line-width", this.readableWidth ? "800px" : "none");
  }
}

export const settings = new Settings();
