import { toggleTask, type TaskItem } from "./api";
import { localDate } from "./dates";

export const taskKey = (t: TaskItem) => `${t.path}:${t.line}`;

/**
 * Shared "check a task" flow: flush unsaved editor changes, toggle the task
 * in its file (the backend refuses if the line changed), then reload.
 * One toggle per task at a time; `isBusy` drives the disabled checkbox.
 */
export function createTaskToggle(opts: {
  beforewrite: () => Promise<void>;
  reload: () => Promise<void>;
  onerror: (message: string) => void;
}) {
  let busy = $state(new Set<string>());

  return {
    isBusy: (t: TaskItem) => busy.has(taskKey(t)),
    async toggle(t: TaskItem) {
      const key = taskKey(t);
      if (busy.has(key)) return;
      busy = new Set(busy).add(key);
      try {
        await opts.beforewrite();
        await toggleTask(t, localDate());
      } catch (e) {
        opts.onerror(String(e));
      } finally {
        const next = new Set(busy);
        next.delete(key);
        busy = next;
        await opts.reload();
      }
    },
  };
}
