// Custom hotkeys: which keys run which command. Changed keys live in the
// vault's `.obsidian/hotkeys.json` (Obsidian's own file and format), so they
// sync with the settings and Obsidian's custom keys work here too.

import { getHotkeys, setHotkey, type Hotkey } from "./api";

const MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

type Mod = Hotkey["modifiers"][number];
const ORDER: Mod[] = ["Mod", "Ctrl", "Meta", "Alt", "Shift"];

/**
 * The key of an event as Obsidian names it. Letters and digits come from
 * the physical key, so Korean input mode (ㅔ for E) doesn't matter. Null
 * for a modifier on its own.
 */
export function eventKey(e: KeyboardEvent): string | null {
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3);
  if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
  if (["Control", "Shift", "Alt", "Meta", "OS", "Process", "Dead", "Unidentified", "CapsLock"].includes(e.key)) return null;
  return e.key.length === 1 ? e.key.toUpperCase() : e.key;
}

function eventMods(e: KeyboardEvent): Set<Mod> {
  const m = new Set<Mod>();
  if (MAC ? e.metaKey : e.ctrlKey) m.add("Mod");
  if (MAC ? e.ctrlKey : e.metaKey) m.add(MAC ? "Ctrl" : "Meta");
  if (e.altKey) m.add("Alt");
  if (e.shiftKey) m.add("Shift");
  return m;
}

/** "Ctrl" on Windows is the same key as "Mod" (and "Meta" on macOS). */
function canonical(m: Mod): Mod {
  if (!MAC && m === "Ctrl") return "Mod";
  if (MAC && m === "Meta") return "Mod";
  return m;
}

export function matches(e: KeyboardEvent, h: Hotkey): boolean {
  const key = eventKey(e);
  if (!key || key.toLowerCase() !== h.key.toLowerCase()) return false;
  const want = new Set(h.modifiers.map(canonical));
  const have = eventMods(e);
  return want.size === have.size && [...want].every((m) => have.has(m));
}

/** The hotkey a key press makes (null for a modifier alone). */
export function fromEvent(e: KeyboardEvent): Hotkey | null {
  const key = eventKey(e);
  if (!key) return null;
  const mods = eventMods(e);
  return { modifiers: ORDER.filter((m) => mods.has(m)), key };
}

/** Plain keys are for typing: a hotkey needs Ctrl, Alt or the like (F-keys excepted). */
export function usable(h: Hotkey): boolean {
  return h.modifiers.some((m) => m !== "Shift") || /^F\d{1,2}$/.test(h.key);
}

/** "Mod+Shift+P" → a hotkey. */
export function hk(spec: string): Hotkey {
  const parts = spec.split("+");
  const key = parts.pop() || "+";
  return { modifiers: parts as Mod[], key };
}

const MOD_NAMES: Record<Mod, string> = MAC
  ? { Mod: "Cmd", Ctrl: "Ctrl", Meta: "Cmd", Alt: "Option", Shift: "Shift" }
  : { Mod: "Ctrl", Ctrl: "Ctrl", Meta: "Win", Alt: "Alt", Shift: "Shift" };
const KEY_NAMES: Record<string, string> = {
  " ": "Space", ArrowLeft: "←", ArrowRight: "→", ArrowUp: "↑", ArrowDown: "↓", Escape: "Esc",
};

/** How a hotkey is written for people: "Ctrl+Shift+P". */
export function label(h: Hotkey): string {
  const mods = [...new Set(h.modifiers.map(canonical))].sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
  return [...mods.map((m) => MOD_NAMES[m]), KEY_NAMES[h.key] ?? h.key].join("+");
}

export const same = (a: Hotkey, b: Hotkey) => label(a) === label(b);

class Hotkeys {
  /** Commands whose keys were changed (by command id); [] means none. */
  custom = $state<Record<string, Hotkey[]>>({});

  /** Read the vault's hotkeys.json (none when no vault is open). */
  async load() {
    try {
      this.custom = await getHotkeys();
    } catch {
      this.custom = {};
    }
  }

  /** The keys of command `id`, given its defaults. */
  keysFor(id: string, defaults: Hotkey[]): Hotkey[] {
    return this.custom[id] ?? defaults;
  }

  /** Change the keys of `id` (null: back to the defaults). */
  async set(id: string, keys: Hotkey[] | null) {
    await setHotkey(id, keys);
    const next = { ...this.custom };
    if (keys) next[id] = keys;
    else delete next[id];
    this.custom = next;
  }
}

export const hotkeys = new Hotkeys();
