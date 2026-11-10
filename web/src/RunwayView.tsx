// The runway view: one lane per runway, every landing and take-off as a
// mark, and bands where a runway was closed, under inspection or not in use.
// Drawn on a canvas; React only supplies the frame around it.
import { useEffect, useRef, useState } from "react";
import { type Band, type Mark, type Timeline, clock } from "./timeline";

const AXIS_H = 46;
const LANE_H = 68;
const DIVERT_H = 30;
const WAIT_H = 64;
const PAD = 20; // room at both ends so edge labels are not clipped
const MS_PER_HOUR = 3_600_000;

interface Hit {
  x: number;
  y: number;
  lines: string[];
}

interface Target {
  x0: number;
  x1: number;
  y0: number;
  y1: number;
  lines: string[];
  /** Marks win over the bands behind them. */
  rank: number;
}

function cssColors(el: Element) {
  const style = getComputedStyle(el);
  const get = (name: string) => style.getPropertyValue(name).trim();
  return {
    surface: get("--surface-1"),
    unused: get("--surface-2"),
    text: get("--text-primary"),
    secondary: get("--text-secondary"),
    muted: get("--text-muted"),
    grid: get("--grid"),
    arrival: get("--series-1"),
    departure: get("--series-2"),
    critical: get("--status-critical"),
    hatch: get("--hatch"),
  };
}

function markLines(m: Mark): string[] {
  const verb = m.kind === "arrival" ? "landed on" : "departed from";
  const lines = [
    `${m.flight} · ${m.aircraft}`,
    `${verb} ${m.end} at ${clock(m.atMs)}`,
    `on the runway ${Math.round((m.vacatedMs - m.atMs) / 1000)} s · delayed ${Math.round(m.delayMs / 1000)} s`,
  ];
  if (m.emergency) lines.push(m.emergency === "mayday" ? "MAYDAY" : "PAN-PAN");
  return lines;
}

function bandLines(b: Band, where: string): string[] {
  return [`${where}: ${b.label}`, `${clock(b.fromMs)} to ${clock(b.toMs)}`];
}

/** Draws the timeline and returns the hover targets. Sizes are CSS pixels. */
function draw(canvas: HTMLCanvasElement, t: Timeline, width: number, height: number): Target[] {
  const c = cssColors(canvas);
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);
  const g = canvas.getContext("2d");
  if (!g) return [];
  g.scale(dpr, dpr);
  g.fillStyle = c.surface;
  g.fillRect(0, 0, width, height);
  g.font = "11px system-ui, sans-serif";
  g.textBaseline = "middle";

  const targets: Target[] = [];
  const x = (ms: number) => PAD + (ms / t.endMs) * (width - 2 * PAD);
  const lanesTop = AXIS_H;
  const lanesBottom = lanesTop + t.lanes.length * LANE_H;
  const divertTop = lanesBottom;
  const waitTop = divertTop + DIVERT_H;

  const hatch = (x0: number, y0: number, w: number, h: number, rising: boolean) => {
    g.save();
    g.beginPath();
    g.rect(x0, y0, w, h);
    g.clip();
    g.strokeStyle = c.hatch;
    g.lineWidth = 1;
    g.beginPath();
    for (let d = -h; d < w + h; d += 7) {
      if (rising) {
        g.moveTo(x0 + d, y0 + h);
        g.lineTo(x0 + d + h, y0);
      } else {
        g.moveTo(x0 + d, y0);
        g.lineTo(x0 + d + h, y0 + h);
      }
    }
    g.stroke();
    g.restore();
  };
  const bandLabel = (text: string, x0: number, w: number, y: number) => {
    if (g.measureText(text).width + 12 > w) return;
    const tw = g.measureText(text).width;
    g.fillStyle = c.surface;
    g.fillRect(x0 + 4, y - 8, tw + 8, 16);
    g.fillStyle = c.secondary;
    g.textAlign = "left";
    g.fillText(text, x0 + 8, y);
  };

  // Time axis: a tick each hour, a faint one each quarter when there is room.
  const pxPerHour = ((width - 2 * PAD) / t.endMs) * MS_PER_HOUR;
  const minor = pxPerHour >= 240 ? MS_PER_HOUR / 4 : pxPerHour >= 110 ? MS_PER_HOUR / 2 : MS_PER_HOUR;
  for (let ms = 0; ms <= t.endMs; ms += minor) {
    const major = ms % MS_PER_HOUR === 0;
    g.strokeStyle = c.grid;
    g.lineWidth = 1;
    g.globalAlpha = major ? 1 : 0.5;
    g.beginPath();
    g.moveTo(Math.round(x(ms)) + 0.5, AXIS_H - (major ? 6 : 3));
    g.lineTo(Math.round(x(ms)) + 0.5, height);
    g.stroke();
    g.globalAlpha = 1;
    if (major || pxPerHour >= 240) {
      g.fillStyle = c.muted;
      g.textAlign = "center";
      g.fillText(clock(ms).slice(0, 5), x(ms), AXIS_H - 14);
    }
  }
  // Wind changes sit above the hours.
  let lastNoteEnd = -Infinity;
  for (const note of t.notes) {
    const nx = Math.max(x(note.atMs), lastNoteEnd + 10);
    g.fillStyle = c.secondary;
    g.textAlign = "left";
    g.fillText(`▾ ${note.text}`, nx - 4, 10);
    lastNoteEnd = nx + g.measureText(`▾ ${note.text}`).width;
  }

  t.lanes.forEach((lane, i) => {
    const top = lanesTop + i * LANE_H;
    const mid = top + LANE_H / 2;
    for (const b of lane.bands) {
      const x0 = x(b.fromMs);
      const w = Math.max(1, x(b.toMs) - x0);
      if (b.kind === "unused") {
        g.fillStyle = c.unused;
        g.fillRect(x0, top + 4, w, LANE_H - 8);
      } else {
        g.fillStyle = c.unused;
        g.fillRect(x0, top + 4, w, LANE_H - 8);
        hatch(x0, top + 4, w, LANE_H - 8, b.kind === "closed");
      }
      bandLabel(b.label, x0, w, b.kind === "unused" ? top + 16 : mid);
      targets.push({ x0, x1: x0 + w, y0: top + 4, y1: top + LANE_H - 4, lines: bandLines(b, lane.name), rank: 0 });
    }
    g.strokeStyle = c.grid;
    g.lineWidth = 1;
    g.beginPath();
    g.moveTo(PAD, Math.round(mid) + 0.5);
    g.lineTo(width - PAD, Math.round(mid) + 0.5);
    g.stroke();

    for (const m of lane.marks) {
      const x0 = x(m.atMs);
      const w = Math.max(2, x(m.vacatedMs) - x0 - 1);
      const up = m.kind === "arrival";
      g.fillStyle = up ? c.arrival : c.departure;
      const y0 = up ? mid - 21 : mid + 2;
      g.fillRect(x0, y0, w, 19);
      if (m.emergency) {
        // Status colour plus a shape plus a label: never colour alone.
        const tip = up ? y0 - 3 : y0 + 22;
        const base = up ? tip - 8 : tip + 8;
        g.fillStyle = c.critical;
        g.beginPath();
        g.moveTo(x0 + 1, tip);
        g.lineTo(x0 - 4, base);
        g.lineTo(x0 + 6, base);
        g.closePath();
        g.fill();
        g.fillStyle = c.text;
        g.textAlign = "left";
        g.fillText(`${m.emergency === "mayday" ? "MAYDAY" : "PAN-PAN"} ${m.flight}`, x0 + 10, (tip + base) / 2);
      }
      targets.push({ x0: x0 - 2, x1: x0 + w + 2, y0, y1: y0 + 19, lines: markLines(m), rank: 1 });
    }
  });

  // Ground stops cover every runway.
  for (const b of t.stops) {
    const x0 = x(b.fromMs);
    const w = Math.max(1, x(b.toMs) - x0);
    hatch(x0, lanesTop + 4, w, lanesBottom - lanesTop - 8, true);
    bandLabel(b.label, x0, w, lanesTop + 16);
    targets.push({ x0, x1: x0 + w, y0: lanesTop, y1: lanesBottom, lines: bandLines(b, "All runways"), rank: 0 });
  }

  // Diversions.
  const dy = divertTop + DIVERT_H / 2;
  g.strokeStyle = c.text;
  g.lineWidth = 1.5;
  for (const d of t.diversions) {
    const dx = x(d.atMs);
    g.beginPath();
    g.moveTo(dx - 4, dy - 4);
    g.lineTo(dx + 4, dy + 4);
    g.moveTo(dx + 4, dy - 4);
    g.lineTo(dx - 4, dy + 4);
    g.stroke();
    targets.push({
      x0: dx - 6,
      x1: dx + 6,
      y0: dy - 8,
      y1: dy + 8,
      lines: [d.flight, `diverted at ${clock(d.atMs)}`, "held too long without a slot"],
      rank: 1,
    });
  }

  // Waiting flights: a stepped area, zero at the bottom.
  const base = waitTop + WAIT_H - 8;
  const scale = (WAIT_H - 22) / Math.max(1, t.maxWaiting);
  g.beginPath();
  g.moveTo(x(0), base);
  let level = 0;
  for (const step of t.waiting) {
    g.lineTo(x(step.atMs), base - level * scale);
    level = step.waiting;
    g.lineTo(x(step.atMs), base - level * scale);
  }
  g.lineTo(x(t.endMs), base - level * scale);
  g.lineTo(x(t.endMs), base);
  g.closePath();
  g.fillStyle = c.muted;
  g.globalAlpha = 0.35;
  g.fill();
  g.globalAlpha = 1;
  g.strokeStyle = c.secondary;
  g.lineWidth = 1.5;
  g.stroke();
  const peak = t.waiting.find((s) => s.waiting === t.maxWaiting);
  if (peak && t.maxWaiting > 0) {
    g.fillStyle = c.text;
    g.textAlign = x(peak.atMs) > width - 90 ? "right" : "left";
    g.fillText(`peak ${t.maxWaiting}`, x(peak.atMs) + (g.textAlign === "left" ? 4 : -4), base - t.maxWaiting * scale - 7);
  }
  targets.push({ x0: 0, x1: width, y0: waitTop, y1: waitTop + WAIT_H, lines: [], rank: -1 });

  return targets;
}

const ZOOMS = [1, 2, 4, 8];

export function TimelineView({ timeline }: { timeline: Timeline }) {
  const scroller = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const targets = useRef<Target[]>([]);
  const [zoom, setZoom] = useState(1);
  const [fit, setFit] = useState(900);
  const [hit, setHit] = useState<Hit | undefined>();
  const [theme, setTheme] = useState(0);

  const width = Math.max(320, fit * zoom);
  const height = AXIS_H + timeline.lanes.length * LANE_H + DIVERT_H + WAIT_H;

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setFit(el.clientWidth));
    observer.observe(el);
    const dark = window.matchMedia("(prefers-color-scheme: dark)");
    const onTheme = () => setTheme((n) => n + 1);
    dark.addEventListener("change", onTheme);
    return () => {
      observer.disconnect();
      dark.removeEventListener("change", onTheme);
    };
  }, []);

  useEffect(() => {
    if (canvas.current) targets.current = draw(canvas.current, timeline, width, height);
  }, [timeline, width, height, theme]);

  const onMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - box.left;
    const py = e.clientY - box.top;
    let best: Target | undefined;
    for (const t of targets.current) {
      if (px < t.x0 || px > t.x1 || py < t.y0 || py > t.y1) continue;
      if (!best || t.rank >= best.rank) best = t;
    }
    let lines = best?.lines ?? [];
    if (best?.rank === -1) {
      // Over the waiting row: read the count at this moment.
      const ms = ((px - PAD) / (width - 2 * PAD)) * timeline.endMs;
      let waiting = 0;
      for (const step of timeline.waiting) {
        if (step.atMs > ms) break;
        waiting = step.waiting;
      }
      lines = ms >= 0 && ms <= timeline.endMs ? [`${waiting} waiting at ${clock(ms)}`] : [];
    }
    const outer = scroller.current?.getBoundingClientRect();
    setHit(lines.length && outer ? { x: e.clientX - outer.left, y: e.clientY - outer.top, lines } : undefined);
  };

  const rows: [string, number, string?][] = [
    ["", AXIS_H],
    ...timeline.lanes.map((l): [string, number, string] => [l.name, LANE_H, "runway"]),
    ["Diverted", DIVERT_H],
    ["Waiting", WAIT_H],
  ];

  return (
    <section className="timeline" aria-label="Runway timeline">
      <div className="timeline-bar">
        <ul className="legend">
          <li>
            <i className="swatch arrival" /> Landing (above the line)
          </li>
          <li>
            <i className="swatch departure" /> Take-off (below the line)
          </li>
          <li>
            <i className="swatch closed" /> Closed
          </li>
          <li>
            <i className="swatch inspection" /> Inspection
          </li>
          <li>
            <i className="swatch unused" /> Not in use
          </li>
          <li>
            <i className="swatch emergency" /> Emergency
          </li>
        </ul>
        <label>
          Zoom{" "}
          <select value={zoom} onChange={(e) => setZoom(Number(e.target.value))}>
            {ZOOMS.map((z) => (
              <option key={z} value={z}>
                {z}×
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="timeline-body">
        <div className="lane-labels">
          {rows.map(([name, h, kind], i) => (
            <div key={i} style={{ height: h }} className={kind}>
              {name}
            </div>
          ))}
        </div>
        <div className="timeline-scroll" ref={scroller} onMouseLeave={() => setHit(undefined)}>
          <canvas
            ref={canvas}
            style={{ width, height }}
            onMouseMove={onMove}
            role="img"
            aria-label="Landings, take-offs and closures on each runway over time. The same data is in the table below."
          />
          {hit && (
            <div
              className="tooltip"
              style={{
                left: Math.min(hit.x + 12, Math.max(0, fit - 230)) + (scroller.current?.scrollLeft ?? 0),
                top: hit.y + 14,
              }}
            >
              {hit.lines.map((line, i) => (
                <div key={i} className={i === 0 ? "tooltip-title" : undefined}>
                  {line}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
