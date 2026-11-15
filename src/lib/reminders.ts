// Task reminders as system notifications while the app is open. Every 30 s
// the app asks for reminders in the last few minutes (so a short sleep or a
// restart doesn't lose one) and notifies about each one once.

import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { fixedReminders, taskReminders, type FixedReminder, type Reminder } from "./api";
import { localDate } from "./dates";

const POLL_MS = 30_000;
/** Reminders this old are still shown (app was closed or asleep). */
const LOOKBACK_MS = 10 * 60_000;
const FIRED_KEY = "rusidian.reminders.fired";
const FIRED_MAX = 500;
/** Fixed expenses read every record, so they are checked less often. */
const FIXED_POLL_MS = 15 * 60_000;

const pad = (n: number) => String(n).padStart(2, "0");
const stamp = (d: Date) =>
  `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** Includes the line text, so changing a reminder lets it fire again. */
const keyOf = (r: Reminder) => `${r.path}\u0000${r.raw}\u0000${r.at}`;
/** Once before the due day and once after it, per payment. */
const fixedKey = (r: FixedReminder) => `fixed\u0000${r.name}\u0000${r.due}\u0000${r.days < 0 ? "late" : "soon"}`;

const won = (cents: number) => `${Math.round(cents / 100).toLocaleString("ko-KR")}원`;

function fixedText(r: FixedReminder): { title: string; body: string } {
  const [, m, d] = r.due.split("-").map(Number);
  const when = `${m}/${d}`;
  if (r.days < 0) return { title: `고정지출 · ${r.name}`, body: `${when} 결제일이 지났는데 기록이 없습니다 (${won(r.amount)}).` };
  if (r.days === 0) return { title: `고정지출 · ${r.name}`, body: `오늘(${when}) ${won(r.amount)}이 나가는 날입니다.` };
  return { title: `고정지출 · ${r.name}`, body: `${r.days}일 뒤 ${when}에 ${won(r.amount)}이 나갑니다.` };
}

function loadFired(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(FIRED_KEY) ?? "[]");
    return Array.isArray(v) ? v : [];
  } catch {
    return [];
  }
}

function saveFired(keys: string[]) {
  try {
    localStorage.setItem(FIRED_KEY, JSON.stringify(keys.slice(-FIRED_MAX)));
  } catch {
    // Without storage a reminder may repeat after a restart; harmless.
  }
}

let permission: boolean | null = null;

async function allowed(): Promise<boolean> {
  if (permission !== null) return permission;
  try {
    permission = (await isPermissionGranted()) || (await requestPermission()) === "granted";
  } catch {
    permission = false;
  }
  return permission;
}

/** Start polling; `enabled` is checked on every round. Returns a stop function. */
export function startReminders(enabled: () => boolean): () => void {
  let running = false;
  let fixedAt = 0;

  async function check() {
    if (running || !enabled()) return;
    running = true;
    try {
      const now = new Date();
      const due = await taskReminders(stamp(new Date(now.getTime() - LOOKBACK_MS)), stamp(now));
      let fixed: FixedReminder[] = [];
      if (now.getTime() - fixedAt >= FIXED_POLL_MS) {
        fixedAt = now.getTime();
        fixed = await fixedReminders(localDate()).catch(() => []);
      }
      if (!due.length && !fixed.length) return;
      const fired = loadFired();
      const fresh = due.filter((r) => !fired.includes(keyOf(r)));
      const freshFixed = fixed.filter((r) => !fired.includes(fixedKey(r)));
      if ((!fresh.length && !freshFixed.length) || !(await allowed())) return;
      for (const r of fresh) {
        sendNotification({ title: `할일 알림 · ${r.at.slice(11)}`, body: `${r.text}\n${r.title}` });
        fired.push(keyOf(r));
      }
      for (const r of freshFixed) {
        sendNotification(fixedText(r));
        fired.push(fixedKey(r));
      }
      saveFired(fired);
    } catch {
      // No vault open, or the vault is being reopened: try again next round.
    } finally {
      running = false;
    }
  }

  check();
  const timer = setInterval(check, POLL_MS);
  return () => clearInterval(timer);
}
