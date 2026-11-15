// Math (KaTeX) and diagrams (Mermaid) for the reading view and live
// preview. Both libraries are big, so they load on first use only; until
// then the source text shows dimmed in place.

import type katexType from "katex";
import { settings } from "./settings.svelte";

let katex: typeof katexType | null = null;
let katexLoading: Promise<typeof katexType> | null = null;

function loadKatex(): Promise<typeof katexType> {
  katexLoading ??= Promise.all([import("katex"), import("katex/dist/katex.min.css")]).then(([m]) => {
    katex = m.default;
    return katex;
  });
  return katexLoading;
}

/** Render TeX into `el` (now if KaTeX is loaded, else as soon as it is). Errors show in red, never throw. */
export function renderMath(el: HTMLElement, tex: string, display: boolean) {
  el.classList.add(display ? "math-block" : "math-inline");
  const draw = (k: typeof katexType) => {
    try {
      k.render(tex, el, { displayMode: display, throwOnError: false, strict: "ignore", trust: false, output: "htmlAndMathml" });
    } catch (e) {
      el.textContent = tex;
      el.title = String(e);
      el.classList.add("math-error");
    }
  };
  if (katex) {
    draw(katex);
    return;
  }
  el.textContent = display ? `$$${tex}$$` : `$${tex}$`;
  el.classList.add("math-pending");
  loadKatex().then((k) => {
    el.classList.remove("math-pending");
    draw(k);
  });
}

/**
 * Inline `$…$` in a line of text: opening `$` not followed by a space,
 * closing `$` not preceded by one nor followed by a digit (so "$5 and $10"
 * stays text), no `$$`, `\$` escapes. Group 1 is the TeX.
 */
export const INLINE_MATH = /(?<![\\$])\$(?![\s$])((?:\\.|[^$\\\n])+?)(?<![\s\\])\$(?![\d$])/g;

// ------------------------------------------------------------------ mermaid

type Mermaid = typeof import("mermaid").default;
let mermaidLoading: Promise<Mermaid> | null = null;
let mermaidTheme = "";
let seq = 0;

function loadMermaid(light: boolean): Promise<Mermaid> {
  mermaidLoading ??= import("mermaid").then((m) => m.default);
  return mermaidLoading.then((m) => {
    const theme = settings.resolved === "dark" && !light ? "dark" : "default";
    if (theme !== mermaidTheme) {
      // strict: no scripts or click handlers from the diagram text
      m.initialize({ startOnLoad: false, securityLevel: "strict", theme, fontFamily: "inherit" });
      mermaidTheme = theme;
    }
    return m;
  });
}

/**
 * Draw a Mermaid diagram into `el`; a syntax error shows the message and the
 * source. `light` draws in light colours whatever the theme (for printing).
 */
export async function renderMermaid(el: HTMLElement, code: string, light = false) {
  el.classList.add("mermaid-block");
  el.textContent = code;
  el.classList.add("mermaid-pending");
  const id = `rusidian-mermaid-${++seq}`;
  try {
    const m = await loadMermaid(light);
    const { svg } = await m.render(id, code);
    el.innerHTML = svg;
  } catch (e) {
    // A failed render can leave its scratch element at the end of <body>.
    document.getElementById(`d${id}`)?.remove();
    document.getElementById(id)?.remove();
    el.textContent = "";
    const msg = document.createElement("div");
    msg.className = "mermaid-error";
    msg.textContent = `다이어그램을 그릴 수 없습니다: ${String((e as Error)?.message ?? e).split("\n")[0]}`;
    const pre = document.createElement("pre");
    pre.textContent = code;
    el.append(msg, pre);
  } finally {
    el.classList.remove("mermaid-pending");
  }
}
