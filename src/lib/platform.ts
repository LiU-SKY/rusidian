// Which kind of device the app runs on. The phone build has one window, no
// file manager to show files in, no Everything and no self-update.

/** Android or iOS. */
export const MOBILE = typeof navigator !== "undefined" && /Android|iPhone|iPad/i.test(navigator.userAgent);

/** For a command's `when`: only on the desktop app. */
export const onDesktop = (when?: () => boolean) => () => !MOBILE && (when ? when() : true);
