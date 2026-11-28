/**
 * The Map's drawing: subjects as circles that settle like springs, lines
 * between subjects that come up together, zoom and drag. Drawn on a canvas
 * by the force-graph library; every color is read from the app's own theme
 * variables, so a theme needs no drawing code of its own.
 */
import type { MapLink, MapProject, MapSubject } from "./memoryMap";

export interface MapData {
  subjects: MapSubject[];
  links: MapLink[];
  projects: MapProject[];
  /** The project whose name stands at the center; About you otherwise. */
  centerProject?: { id: string; name: string } | null;
}

export interface MapHandle {
  setData(data: MapData): void;
  select(id: string | null, glide?: boolean): void;
  zoomBy(factor: number): void;
  fit(ms?: number): void;
  resize(): void;
  /** Where each circle is on the screen now: for pressing one without a pointer. */
  placed(): { id: string; name: string; x: number; y: number }[];
  destroy(): void;
}

interface Node {
  id: string;
  name: string;
  items: number;
  audience: MapSubject["audience"];
  color: string;
  you: boolean;
  center: boolean;
  suggested: boolean;
  born: number;
  x: number;
  y: number;
  vx?: number;
  vy?: number;
  fx?: number;
  fy?: number;
}
type Link = { source: string | Node; target: string | Node; w: number };

const TAU = Math.PI * 2;
const PROJECT_ID = "project:";

function rgb(color: string): [number, number, number] {
  let h = color.trim().replace("#", "");
  if (h.length === 3) h = h.replace(/./g, "$&$&");
  const n = parseInt(h.slice(0, 6), 16);
  return Number.isFinite(n) && h.length >= 6 ? [(n >> 16) & 255, (n >> 8) & 255, n & 255] : [128, 128, 128];
}
const rgba = (color: string, a: number) => {
  const [r, g, b] = rgb(color);
  return `rgba(${r},${g},${b},${a})`;
};
const mix = (a: string, b: string, t: number) => {
  const [x, y] = [rgb(a), rgb(b)];
  return `rgb(${[0, 1, 2].map((i) => Math.round(x[i] + (y[i] - x[i]) * t)).join(",")})`;
};

/** A circle's size from how much it holds. */
function radius(n: Node): number {
  if (n.center && !n.you) return 30;
  const r = 7 + Math.sqrt(Math.max(n.items, 1)) * 4.3;
  return n.you && !n.center ? r * 0.7 : r;
}

export async function mountMap(el: HTMLElement, on: { select: (id: string | null) => void }): Promise<MapHandle> {
  const { default: ForceGraph } = await import("force-graph");
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const nodes = new Map<string, Node>();
  let selected: string | null = null;
  let hover: string | null = null;
  let near: Set<string> | null = null;
  let alive = true;

  // The theme: read once, and again whenever it changes.
  let C = { bg: "#000", card: "#222", primary: "#fff", text: "#fff", muted: "#888", second: "#aaa", none: "#38bdf8", quiet: "#a1a1aa", projects: [] as string[] };
  const readTheme = () => {
    const s = getComputedStyle(el);
    const v = (name: string, fallback: string) => s.getPropertyValue(name).trim() || fallback;
    C = {
      bg: v("--bg-main", "#000000"),
      card: v("--bg-card", "#27272a"),
      primary: v("--bg-button-primary", "#f5f5f5"),
      text: v("--text-primary", "#ffffff"),
      muted: v("--text-muted", "#97979e"),
      second: v("--text-secondary", "#a1a1a1"),
      none: v("--map-none", "#38bdf8"),
      quiet: v("--map-quiet", "#a1a1aa"),
      projects: [1, 2, 3, 4, 5, 6].map((n) => v(`--map-project-${n}`, "#a1a1aa")),
    };
  };
  readTheme();

  const paint = (n: Node, ctx: CanvasRenderingContext2D, k: number) => {
    const now = performance.now();
    const t = Math.min(Math.max((now - n.born) / 700, 0), 1);
    const grow = reduced ? 1 : 1 + 2.70158 * Math.pow(t - 1, 3) + 1.70158 * Math.pow(t - 1, 2);
    const breathe = reduced ? 0 : Math.sin(now / 1400 + n.x * 0.01) * 0.018;
    const r = Math.max(radius(n) * grow * (1 + breathe), 0.1);
    const col = n.you ? C.text : n.color;
    const focus = hover || selected;
    const lit = !focus || !!near?.has(n.id);
    const sel = selected === n.id;
    const hov = hover === n.id;
    ctx.save();
    ctx.globalAlpha = lit ? 1 : 0.22;
    const halo = ctx.createRadialGradient(n.x, n.y, r * 0.7, n.x, n.y, r * (sel || hov ? 3 : 2.4));
    halo.addColorStop(0, rgba(col, sel || hov ? 0.5 : 0.32));
    halo.addColorStop(1, rgba(col, 0));
    ctx.fillStyle = halo;
    ctx.beginPath();
    ctx.arc(n.x, n.y, r * 3, 0, TAU);
    ctx.fill();
    const body = ctx.createRadialGradient(n.x - r * 0.35, n.y - r * 0.4, r * 0.05, n.x, n.y, r);
    body.addColorStop(0, mix(col, "#ffffff", 0.55));
    body.addColorStop(0.7, col);
    body.addColorStop(1, mix(col, C.bg, 0.3));
    ctx.fillStyle = body;
    ctx.beginPath();
    ctx.arc(n.x, n.y, r, 0, TAU);
    ctx.fill();
    // The ring says who can use it, by pattern as well as color.
    ctx.setLineDash(n.audience === "ai" || n.audience === "local" ? [6 / k, 4 / k] : n.audience === "project" ? [1.5 / k, 4 / k] : n.audience === "mixed" ? [10 / k, 3 / k, 1.5 / k, 3 / k] : []);
    ctx.lineCap = "round";
    ctx.lineWidth = (sel ? 2.5 : 1.5) / k;
    ctx.strokeStyle = sel ? C.primary : rgba(col, 0.85);
    ctx.beginPath();
    ctx.arc(n.x, n.y, r + 5 / k, 0, TAU);
    ctx.stroke();
    ctx.setLineDash([]);
    // The name, the same size on screen at every zoom; a small subject shows its name once zoomed in.
    if (grow > 0.6 && (sel || hov || r * k > 13 || n.center || n.you)) {
      const fs = 12 / k;
      const y = n.y + r + 9 / k + fs * 0.8;
      ctx.font = `${sel || n.center || n.you ? 600 : 500} ${fs}px 'Plus Jakarta Sans', system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "alphabetic";
      ctx.lineJoin = "round";
      ctx.lineWidth = 4 / k;
      ctx.strokeStyle = C.bg;
      ctx.strokeText(n.name, n.x, y);
      ctx.fillStyle = n.suggested ? C.muted : C.text;
      ctx.fillText(n.name, n.x, y);
      if (!(n.center && !n.you) && (sel || hov || k > 1.5)) {
        ctx.font = `400 ${fs * 0.86}px 'Plus Jakarta Sans', system-ui, sans-serif`;
        const words = `${n.items} ${n.items === 1 ? "item" : "items"}`;
        ctx.strokeText(words, n.x, y + fs * 1.25);
        ctx.fillStyle = C.second;
        ctx.fillText(words, n.x, y + fs * 1.25);
      }
    }
    ctx.restore();
  };

  const end = (x: string | Node) => (typeof x === "object" ? x.id : x);
  const linkLit = (l: Link): boolean | null => {
    const f = hover || selected;
    return f ? end(l.source) === f || end(l.target) === f : null;
  };

  // Two small forces of our own: a pull toward the middle, and no overlapping.
  const gravity = (k: number) => {
    let ns: Node[] = [];
    const f = (alpha: number) => {
      for (const n of ns) {
        n.vx = (n.vx ?? 0) - n.x * k * alpha;
        n.vy = (n.vy ?? 0) - n.y * k * alpha;
      }
    };
    f.initialize = (x: Node[]) => (ns = x);
    return f;
  };
  const apart = (pad: number) => {
    let ns: Node[] = [];
    const f = () => {
      for (let i = 0; i < ns.length; i++)
        for (let j = i + 1; j < ns.length; j++) {
          const [a, b] = [ns[i], ns[j]];
          let [dx, dy] = [b.x - a.x, b.y - a.y];
          const d = Math.sqrt(dx * dx + dy * dy) || 0.01;
          const min = radius(a) + radius(b) + pad;
          if (d >= min) continue;
          const p = ((min - d) / d) * 0.5;
          dx *= p;
          dy *= p;
          if (a.fx == null) [a.x, a.y] = [a.x - dx, a.y - dy];
          if (b.fx == null) [b.x, b.y] = [b.x + dx, b.y + dy];
        }
    };
    f.initialize = (x: Node[]) => (ns = x);
    return f;
  };

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const graph: any = new ForceGraph(el);
  graph
    .nodeId("id")
    .nodeLabel(() => "")
    .nodeCanvasObject(paint)
    .nodeCanvasObjectMode(() => "replace")
    .nodePointerAreaPaint((n: Node, color: string, ctx: CanvasRenderingContext2D) => {
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(n.x, n.y, radius(n) + 6, 0, TAU);
      ctx.fill();
    })
    .linkColor((l: Link) => {
      const lit = linkLit(l);
      return lit === null ? rgba(C.second, 0.38) : lit ? rgba(C.text, 0.8) : rgba(C.second, 0.1);
    })
    .linkWidth((l: Link) => 0.6 + Math.min(l.w, 4) * 0.7)
    .linkDirectionalParticles((l: Link) => (reduced ? 0 : Math.min(l.w, 3)))
    .linkDirectionalParticleWidth(2.6)
    .linkDirectionalParticleSpeed(0.0035)
    .linkDirectionalParticleColor((l: Link) => (typeof l.source === "object" ? (l.source.you ? C.text : l.source.color) : C.none))
    .minZoom(0.35)
    .maxZoom(6)
    .autoPauseRedraw(false)
    .cooldownTime(6000)
    .backgroundColor(C.bg)
    .onNodeHover((n: Node | null) => {
      hover = n ? n.id : null;
      refreshNear();
      el.style.cursor = n ? "pointer" : "grab";
    })
    .onNodeClick((n: Node) => on.select(n.id.startsWith(PROJECT_ID) ? null : n.id))
    .onBackgroundClick(() => on.select(null));
  graph.d3Force("center", null);
  graph.d3Force("gravity", gravity(0.07));
  graph.d3Force("apart", apart(26));
  graph.d3Force("charge").strength((n: Node) => -radius(n) * 16);
  graph
    .d3Force("link")
    .distance((l: Link) => 64 + (typeof l.source === "object" ? radius(l.source) : 16) + (typeof l.target === "object" ? radius(l.target) : 16))
    .strength(0.22);

  function refreshNear() {
    const f = hover || selected;
    if (!f) return void (near = null);
    near = new Set([f]);
    for (const l of graph.graphData().links as Link[]) {
      const [a, b] = [end(l.source), end(l.target)];
      if (a === f) near.add(b);
      if (b === f) near.add(a);
    }
  }
  const resize = () => {
    if (el.clientWidth && el.clientHeight) graph.width(el.clientWidth).height(el.clientHeight);
  };
  const fit = (ms = 600) => {
    if (graph.graphData().nodes.length <= 1) {
      graph.centerAt(0, 8, ms);
      graph.zoom(1.7, ms);
      return;
    }
    graph.zoomToFit(ms, 70);
    setTimeout(() => alive && graph.zoom() > 2 && graph.zoom(2, 300), ms + 30);
  };

  // Nothing draws while the page is hidden.
  const onVisible = () => (document.hidden ? graph.pauseAnimation() : graph.resumeAnimation());
  document.addEventListener("visibilitychange", onVisible);
  const retheme = () => {
    readTheme();
    graph.backgroundColor(C.bg);
    for (const n of nodes.values()) n.color = colorOf.get(n.id) ? colorOf.get(n.id)!() : n.color;
  };
  const colorOf = new Map<string, () => string>();
  const themeWatch = new MutationObserver(retheme);
  themeWatch.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-theme"] });
  themeWatch.observe(document.body, { attributes: true, attributeFilter: ["class", "data-theme"] });
  const sizeWatch = new ResizeObserver(resize);
  sizeWatch.observe(el);
  resize();
  let first = true;
  let fits = 0;

  return {
    setData(data) {
      const slot = new Map(data.projects.map((p) => [p.id, p.color]));
      const pick = (project: string | null) => () => {
        if (!project) return C.none;
        const s = slot.get(project);
        return s == null ? C.quiet : (C.projects[s] ?? C.quiet);
      };
      const center = data.centerProject ? PROJECT_ID + data.centerProject.id : (data.subjects.find((s) => s.kind === "about_you")?.id ?? "");
      const want: { id: string; name: string; items: number; audience: MapSubject["audience"]; you: boolean; suggested: boolean; color: () => string }[] = data.subjects.map((s) => ({
        id: s.id,
        name: s.name,
        items: s.items,
        audience: s.audience,
        you: s.kind === "about_you",
        suggested: !s.nameAccepted && s.kind === "conversation",
        color: pick(s.project),
      }));
      if (data.centerProject) want.unshift({ id: center, name: data.centerProject.name, items: 0, audience: "project", you: false, suggested: false, color: pick(data.centerProject.id) });
      const ids = new Set(want.map((w) => w.id));
      for (const id of [...nodes.keys()]) if (!ids.has(id)) nodes.delete(id);
      colorOf.clear();
      const list: Node[] = want.map((w) => {
        let n = nodes.get(w.id);
        if (!n) {
          const a = Math.random() * TAU;
          n = { id: w.id, name: w.name, items: w.items, audience: w.audience, color: w.color(), you: w.you, center: false, suggested: w.suggested, born: performance.now(), x: Math.cos(a) * 60, y: Math.sin(a) * 60 };
          nodes.set(w.id, n);
        }
        Object.assign(n, { name: w.name, items: w.items, audience: w.audience, color: w.color(), you: w.you, suggested: w.suggested, center: w.id === center });
        [n.fx, n.fy] = w.id === center ? [0, 0] : [undefined, undefined];
        colorOf.set(w.id, w.color);
        return n;
      });
      const links: Link[] = data.links.filter((l) => ids.has(l.a) && ids.has(l.b)).map((l) => ({ source: l.a, target: l.b, w: l.times }));
      if (selected && !ids.has(selected)) selected = null;
      graph.graphData({ nodes: list, links });
      graph.d3ReheatSimulation();
      refreshNear();
      setTimeout(() => alive && fit(first ? 0 : 700), first ? 50 : 650);
      // Once the circles have spread to their places, the whole map is brought into view again.
      const mine = ++fits;
      setTimeout(() => alive && mine === fits && fit(600), 2200);
      first = false;
    },
    select(id, glide) {
      selected = id;
      refreshNear();
      const n = id ? nodes.get(id) : undefined;
      if (n && glide) {
        graph.centerAt(n.x, n.y, 500);
        if (graph.zoom() < 1.3) graph.zoom(1.5, 500);
      }
    },
    zoomBy: (factor) => graph.zoom(graph.zoom() * factor, 250),
    fit,
    resize,
    placed: () => {
      const box = el.getBoundingClientRect();
      return [...nodes.values()].map((n) => {
        const p = graph.graph2ScreenCoords(n.x, n.y);
        return { id: n.id, name: n.name, x: box.left + p.x, y: box.top + p.y };
      });
    },
    destroy() {
      alive = false;
      document.removeEventListener("visibilitychange", onVisible);
      themeWatch.disconnect();
      sizeWatch.disconnect();
      graph._destructor();
      el.replaceChildren();
    },
  };
}
