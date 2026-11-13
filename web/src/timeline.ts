// Turns the outputs of a run into what the runway timeline draws. This only
// rearranges what the airport reported; it decides nothing.
import type { AirportConfig, Emergency, Output, RunResult } from "./sim";

export interface Mark {
  kind: "arrival" | "departure";
  flight: string;
  aircraft: string;
  /** The runway direction used, e.g. "27L". */
  end: string;
  atMs: number;
  vacatedMs: number;
  delayMs: number;
  emergency: Emergency | null;
}

export type BandKind = "closed" | "inspection" | "unused";

export interface Band {
  kind: BandKind;
  fromMs: number;
  toMs: number;
  label: string;
}

export interface Lane {
  /** Full runway name, e.g. "09R/27L". */
  name: string;
  marks: Mark[];
  bands: Band[];
}

export interface Note {
  atMs: number;
  text: string;
}

export interface Timeline {
  endMs: number;
  lanes: Lane[];
  /** Ground stops: nothing moves anywhere. */
  stops: Band[];
  diversions: { flight: string; atMs: number }[];
  /** Wind changes, to label the time axis. */
  notes: Note[];
  /** Flights waiting past their requested time, as steps: from each `atMs` the count is `waiting`. */
  waiting: { atMs: number; waiting: number }[];
  maxWaiting: number;
}

export function buildTimeline(airport: AirportConfig, result: RunResult): Timeline {
  const endMs = result.end.now_ms;
  const laneOf = new Map<string, number>();
  const lanes: Lane[] = airport.runways.map((runway, i) => {
    const name = `${runway.low}/${runway.high}`;
    for (const key of [runway.low, runway.high, name]) laneOf.set(key, i);
    return { name, marks: [], bands: [] };
  });

  // Open intervals, closed off as the matching output arrives.
  const closedSince: (number | undefined)[] = lanes.map(() => undefined);
  const closedWhy: string[] = lanes.map(() => "");
  const unusedSince: (number | undefined)[] = result.start.runways.map((r) =>
    r.active_end === null ? 0 : undefined,
  );
  let stopSince: number | undefined;
  let stopWhy = "";

  const stops: Band[] = [];
  const diversions: Timeline["diversions"] = [];
  const notes: Note[] = [];
  const changes: [number, number][] = [];
  const hold = airport.max_arrival_hold_ms;

  const lane = (runway: string): number => {
    const i = laneOf.get(runway);
    if (i === undefined) throw new Error(`unknown runway ${runway}`);
    return i;
  };
  const band = (i: number, kind: BandKind, fromMs: number, toMs: number, label: string) => {
    if (toMs > fromMs) lanes[i]!.bands.push({ kind, fromMs, toMs, label });
  };
  const setUsed = (i: number, used: boolean, atMs: number) => {
    const since = unusedSince[i];
    if (used && since !== undefined) {
      band(i, "unused", since, atMs, "not in use");
      unusedSince[i] = undefined;
    } else if (!used && since === undefined) {
      unusedSince[i] = atMs;
    }
  };

  for (const o of result.outputs as Output[]) {
    switch (o.type) {
      case "landed":
      case "departed":
        lanes[lane(o.runway)]!.marks.push({
          kind: o.type === "landed" ? "arrival" : "departure",
          flight: o.flight,
          aircraft: o.aircraft,
          end: o.runway,
          atMs: o.at_ms,
          vacatedMs: o.vacated_ms,
          delayMs: o.delay_ms,
          emergency: o.emergency,
        });
        changes.push([o.at_ms - o.delay_ms, 1], [o.at_ms, -1]);
        break;
      case "diverted":
        // Only a hold limit makes a flight divert.
        if (hold === null) throw new Error(`${o.flight} diverted at an airport with no hold limit`);
        diversions.push({ flight: o.flight, atMs: o.at_ms });
        changes.push([o.at_ms - hold, 1], [o.at_ms, -1]);
        break;
      case "runway_closed": {
        const i = lane(o.runway);
        if (closedSince[i] === undefined) {
          closedSince[i] = o.at_ms;
          closedWhy[i] = o.reason;
        }
        break;
      }
      case "runway_opened": {
        const i = lane(o.runway);
        if (o.open && closedSince[i] !== undefined) {
          band(i, "closed", closedSince[i], o.at_ms, `closed: ${closedWhy[i]}`);
          closedSince[i] = undefined;
        }
        break;
      }
      case "runway_inspection":
        band(lane(o.runway), "inspection", o.from_ms, o.until_ms, "inspection after mayday");
        break;
      case "active_runways": {
        const used = new Set(o.runways.map(lane));
        lanes.forEach((_, i) => setUsed(i, used.has(i), o.at_ms));
        break;
      }
      case "ground_stop_started":
        if (stopSince === undefined) {
          stopSince = o.at_ms;
          stopWhy = o.reason;
        }
        break;
      case "ground_stop_lifted":
        if (o.clear && stopSince !== undefined) {
          stops.push({ kind: "closed", fromMs: stopSince, toMs: o.at_ms, label: `ground stop: ${stopWhy}` });
          stopSince = undefined;
        }
        break;
      case "wind_changed":
        notes.push({
          atMs: o.at_ms,
          text: `wind ${String(o.direction_deg).padStart(3, "0")}°/${o.speed_kt} kt`,
        });
        break;
      default:
        break;
    }
  }

  // Whatever is still open runs to the end.
  lanes.forEach((_, i) => {
    if (closedSince[i] !== undefined) band(i, "closed", closedSince[i], endMs, `closed: ${closedWhy[i]}`);
    setUsed(i, true, endMs);
  });
  if (stopSince !== undefined) {
    stops.push({ kind: "closed", fromMs: stopSince, toMs: endMs, label: `ground stop: ${stopWhy}` });
  }
  // A closed runway is also "not in use"; show only the closure.
  for (const l of lanes) {
    const closed = l.bands.filter((b) => b.kind === "closed");
    l.bands = l.bands.flatMap((b) => (b.kind === "unused" ? subtract(b, closed) : [b]));
  }

  changes.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const waiting: Timeline["waiting"] = [];
  let count = 0;
  let maxWaiting = 0;
  for (const [atMs, delta] of changes) {
    // Once every change at the previous instant is in, nobody can have
    // stopped waiting before they started.
    const last = waiting[waiting.length - 1];
    if (last && last.atMs !== atMs && last.waiting < 0) {
      throw new Error(`${last.waiting} flights waiting at ${clock(last.atMs)}`);
    }
    count += delta;
    maxWaiting = Math.max(maxWaiting, count);
    if (last && last.atMs === atMs) last.waiting = count;
    else waiting.push({ atMs, waiting: count });
  }
  const final = waiting[waiting.length - 1];
  if (final && final.waiting < 0) throw new Error(`${final.waiting} flights waiting at the end`);

  return { endMs, lanes, stops, diversions, notes, waiting, maxWaiting };
}

/** The parts of `band` not covered by any of `holes`. */
export function subtract(band: Band, holes: Band[]): Band[] {
  let parts = [band];
  for (const hole of holes) {
    parts = parts.flatMap((p) => {
      if (hole.toMs <= p.fromMs || hole.fromMs >= p.toMs) return [p];
      const out: Band[] = [];
      if (hole.fromMs > p.fromMs) out.push({ ...p, toMs: hole.fromMs });
      if (hole.toMs < p.toMs) out.push({ ...p, fromMs: hole.toMs });
      return out;
    });
  }
  return parts;
}

/** `hh:mm:ss` for a simulated time. */
export function clock(ms: number): string {
  const s = Math.floor(ms / 1000);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(Math.floor(s / 3600))}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}`;
}
