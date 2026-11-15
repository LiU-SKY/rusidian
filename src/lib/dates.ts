// Local-date helpers. Dates are `YYYY-MM-DD` strings in the user's time zone,
// which compare correctly as plain strings.

export const WEEKDAYS = ["일", "월", "화", "수", "목", "금", "토"];

const pad = (n: number) => String(n).padStart(2, "0");

export function ymd(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Parse `YYYY-MM-DD` as a local date (not UTC). */
export function parseYmd(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y, m - 1, d);
}

export function addDays(s: string, n: number): string {
  const d = parseYmd(s);
  d.setDate(d.getDate() + n);
  return ymd(d);
}

/** Today (plus `offsetDays`) as YYYY-MM-DD. */
export function localDate(offsetDays = 0): string {
  return addDays(ymd(new Date()), offsetDays);
}

export function localTime(): string {
  const d = new Date();
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function daysBetween(from: string, to: string): number {
  return Math.round((parseYmd(to).getTime() - parseYmd(from).getTime()) / 86_400_000);
}

/** "오늘", "내일", "3일 지남", "11/16 (월)", "12/23", "2027/1/5". */
export function dayLabel(date: string, today: string): string {
  const diff = daysBetween(today, date);
  if (diff === 0) return "오늘";
  if (diff === 1) return "내일";
  if (diff === -1) return "어제";
  if (diff < 0) return `${-diff}일 지남`;
  const d = parseYmd(date);
  const md = `${d.getMonth() + 1}/${d.getDate()}`;
  if (diff <= 7) return `${md} (${WEEKDAYS[d.getDay()]})`;
  return d.getFullYear() === parseYmd(today).getFullYear() ? md : `${d.getFullYear()}/${md}`;
}

/** "11월 13일 (금)" */
export function longLabel(date: string): string {
  const d = parseYmd(date);
  return `${d.getMonth() + 1}월 ${d.getDate()}일 (${WEEKDAYS[d.getDay()]})`;
}
