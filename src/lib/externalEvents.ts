// External calendar events in local time, for the calendar views.

import type { CalendarSource, ExternalEvent } from "./api";
import { addDays, ymd } from "./dates";

export interface LocalEvent {
  key: string;
  title: string;
  location: string | null;
  description: string | null;
  calendarName: string;
  color: string;
  /** All-day, or spanning more than one local day (shown in the all-day row). */
  allDay: boolean;
  /** First and last local day it touches (`YYYY-MM-DD`, inclusive). */
  startDate: string;
  endDate: string;
  /** `HH:MM` local, for timed events. */
  startTime: string | null;
  endTime: string | null;
}

const pad = (n: number) => String(n).padStart(2, "0");
const hhmm = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** Offset of `tz` from UTC at instant `utcMs`, in ms. */
function zoneOffset(utcMs: number, tz: string): number {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: tz, hourCycle: "h23", year: "numeric", month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit", second: "2-digit",
  }).formatToParts(new Date(utcMs));
  const v = Object.fromEntries(parts.map((p) => [p.type, Number(p.value)]));
  return Date.UTC(v.year, v.month - 1, v.day, v.hour, v.minute, v.second) - utcMs;
}

/** `YYYY-MM-DDTHH:MM:SS` wall time in `tz` (null = local) as an instant. */
export function zonedTime(naive: string, tz: string | null): Date {
  const [d, t = "00:00:00"] = naive.split("T");
  const [y, mo, da] = d.split("-").map(Number);
  const [h, mi, s] = t.split(":").map(Number);
  if (!tz) return new Date(y, mo - 1, da, h, mi, s || 0);
  const guess = Date.UTC(y, mo - 1, da, h, mi, s || 0);
  try {
    // Two passes get the offset right around daylight-saving changes.
    let ms = guess - zoneOffset(guess, tz);
    ms = guess - zoneOffset(ms, tz);
    return new Date(ms);
  } catch {
    // Unknown zone name: treat as local time.
    return new Date(y, mo - 1, da, h, mi, s || 0);
  }
}

export function toLocal(e: ExternalEvent, source: CalendarSource | undefined, index: number): LocalEvent {
  const base = {
    key: `${e.calendar}:${e.uid}:${e.start}:${index}`,
    title: e.summary,
    location: e.location,
    description: e.description,
    calendarName: source?.name ?? "캘린더",
    color: source?.color ?? "#2a78d6",
  };
  if (e.all_day) {
    const last = addDays(e.end, -1);
    return { ...base, allDay: true, startDate: e.start, endDate: last < e.start ? e.start : last, startTime: null, endTime: null };
  }
  const start = zonedTime(e.start, e.tz);
  const end = zonedTime(e.end, e.tz);
  // An event ending exactly at midnight doesn't touch the next day.
  const lastMoment = end > start ? new Date(end.getTime() - 1) : start;
  const startDate = ymd(start);
  const endDate = ymd(lastMoment);
  return {
    ...base,
    allDay: endDate > startDate,
    startDate,
    endDate,
    startTime: hhmm(start),
    endTime: end > start ? hhmm(end) : null,
  };
}

export const onDay = (e: LocalEvent, date: string) => e.startDate <= date && date <= e.endDate;

/** "종일", "09:00–10:30", "11/20 – 11/22" */
export function timeLabel(e: LocalEvent): string {
  const md = (s: string) => `${Number(s.slice(5, 7))}/${Number(s.slice(8, 10))}`;
  if (e.startDate !== e.endDate) {
    const from = e.startTime ? `${md(e.startDate)} ${e.startTime}` : md(e.startDate);
    const to = e.startTime && e.endTime ? `${md(e.endDate)} ${e.endTime}` : md(e.endDate);
    return `${from} – ${to}`;
  }
  if (!e.startTime) return "종일";
  return e.endTime ? `${e.startTime}–${e.endTime}` : e.startTime;
}
