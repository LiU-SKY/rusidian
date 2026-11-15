// A highlight that glides to the active item of a menu, tab strip or
// segmented control instead of jumping. Svelte action:
//
//   <nav use:slider={{ selector: ".active" }}> … </nav>
//
// The indicator is a span behind the items (the host gets its own stacking
// context, the span z-index -1), so items should leave their active
// background transparent and let the indicator draw it. It follows the
// active item when classes/ARIA state change or the host resizes, and
// stays put (no animation) on first placement or with reduced motion.

export interface SliderOptions {
  /** Finds the active item inside the host, e.g. ".active" or "[aria-checked='true']". */
  selector: string;
  /** A 2px bar under the item instead of a filled block. */
  underline?: boolean;
  /** Fill (or bar) colour; default var(--active), var(--accent) for underline. */
  color?: string;
  shadow?: string;
}

const EASE = "cubic-bezier(0.3, 0.7, 0.2, 1)";
const MS = 220;

export function slider(host: HTMLElement, options: SliderOptions) {
  let opts = options;
  const bar = document.createElement("span");
  bar.className = "slider-indicator";
  bar.setAttribute("aria-hidden", "true");
  Object.assign(bar.style, {
    position: "absolute",
    left: "0",
    top: "0",
    zIndex: "-1",
    pointerEvents: "none",
    opacity: "0",
  } satisfies Partial<CSSStyleDeclaration>);
  host.prepend(bar);
  if (getComputedStyle(host).position === "static") host.style.position = "relative";
  host.style.isolation = "isolate";

  let placed = false;
  let frame = 0;

  function place() {
    frame = 0;
    const active = [...host.querySelectorAll<HTMLElement>(opts.selector)].find((el) => el !== bar && el.offsetParent === host);
    if (!active) {
      bar.style.opacity = "0";
      placed = false;
      return;
    }
    const still = !placed || matchMedia("(prefers-reduced-motion: reduce)").matches;
    bar.style.transition = still ? "none" : `transform ${MS}ms ${EASE}, width ${MS}ms ${EASE}, height ${MS}ms ${EASE}, opacity 120ms`;
    const x = active.offsetLeft;
    const w = active.offsetWidth;
    if (opts.underline) {
      const y = active.offsetTop + active.offsetHeight - 2;
      Object.assign(bar.style, { width: `${w}px`, height: "2px", borderRadius: "1px", transform: `translate(${x}px, ${y}px)` });
      bar.style.background = opts.color ?? "var(--accent)";
    } else {
      Object.assign(bar.style, {
        width: `${w}px`,
        height: `${active.offsetHeight}px`,
        borderRadius: getComputedStyle(active).borderRadius,
        transform: `translate(${x}px, ${active.offsetTop}px)`,
      });
      bar.style.background = opts.color ?? "var(--active)";
      bar.style.boxShadow = opts.shadow ?? "none";
    }
    bar.style.opacity = "1";
    placed = true;
  }

  const schedule = () => {
    if (!frame) frame = requestAnimationFrame(place);
  };
  const mutations = new MutationObserver(schedule);
  mutations.observe(host, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["class", "aria-selected", "aria-checked", "aria-current", "aria-pressed"],
  });
  const resize = new ResizeObserver(schedule);
  resize.observe(host);
  place();

  return {
    update(next: SliderOptions) {
      opts = next;
      schedule();
    },
    destroy() {
      cancelAnimationFrame(frame);
      mutations.disconnect();
      resize.disconnect();
      bar.remove();
    },
  };
}
