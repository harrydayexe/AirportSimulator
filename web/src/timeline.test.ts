// The timeline only rearranges what the airport reported, so it is tested
// with small hand-made runs and no WebAssembly.
import { expect, test } from "bun:test";
import type { AirportConfig, Output, RunResult, Snapshot } from "./sim";
import { type Band, buildTimeline, clock, subtract } from "./timeline";

const runway = (low: string, high: string) => ({
  low,
  high,
  heading_deg: 90,
  length_m: 3000,
  exit_extra_ms: 0,
  crosswind_limit_kt: 25,
});

const airport: AirportConfig = {
  name: "Test",
  runways: [runway("09L", "27R"), runway("09R", "27L")],
  crossings: [],
  final_approach_nm: 6,
  mayday_inspection_ms: 900_000,
  max_arrival_hold_ms: 2_700_000,
};

function snapshot(nowMs: number, activeEnds: (string | null)[]): Snapshot {
  return {
    now_ms: nowMs,
    wind: { direction_deg: 0, speed_kt: 0 },
    ground_stops: [],
    runways: airport.runways.map((r, i) => ({
      name: `${r.low}/${r.high}`,
      active_end: activeEnds[i] ?? null,
      closures: [],
      inspection_until_ms: null,
    })),
    queue: [],
    stats: {
      accepted: 0,
      landed: 0,
      departed: 0,
      diverted: 0,
      rejected: 0,
      arrival_delay_ms: 0,
      departure_delay_ms: 0,
    },
    fingerprint: "0000000000000000",
  };
}

/** A run of `outputs` ending at `endMs`, with both runways in use from the start unless `start` says otherwise. */
function timeline(outputs: Output[], endMs = 100, start: (string | null)[] = ["09L", "09R"]) {
  const result: RunResult = { outputs, start: snapshot(0, start), end: snapshot(endMs, start) };
  return buildTimeline(airport, result);
}

const span = (b: Band) => [b.kind, b.fromMs, b.toMs];
const byStart = (a: Band, b: Band) => a.fromMs - b.fromMs;

test("clock agrees with the command line", () => {
  expect([0, 999, 3_661_000, 86_399_999, 360_000_000].map(clock)).toEqual([
    "00:00:00",
    "00:00:00",
    "01:01:01",
    "23:59:59",
    "100:00:00",
  ]);
});

test("subtract cuts holes out of a band", () => {
  const band: Band = { kind: "unused", fromMs: 0, toMs: 100, label: "not in use" };
  const hole = (fromMs: number, toMs: number): Band => ({ kind: "closed", fromMs, toMs, label: "" });
  const cut = (...holes: Band[]) => subtract(band, holes).map((b) => [b.fromMs, b.toMs]);
  expect(cut(hole(40, 60))).toEqual([
    [0, 40],
    [60, 100],
  ]);
  expect(cut(hole(0, 100))).toEqual([]);
  expect(cut(hole(-10, 20))).toEqual([[20, 100]]);
  expect(cut(hole(90, 200))).toEqual([[0, 90]]);
  expect(cut(hole(100, 150))).toEqual([[0, 100]]);
  expect(cut(hole(0, 0))).toEqual([[0, 100]]);
  expect(cut(hole(10, 20), hole(30, 40))).toEqual([
    [0, 10],
    [20, 30],
    [40, 100],
  ]);
});

const landed = (runway: string, atMs: number, delayMs: number): Output => ({
  type: "landed",
  flight: "BAW1",
  aircraft: "A320",
  runway,
  at_ms: atMs,
  vacated_ms: atMs + 50,
  delay_ms: delayMs,
  emergency: null,
});

test("a landing becomes a mark on the right lane", () => {
  const t = timeline([landed("27L", 10, 0)]);
  expect(t.lanes[0]!.marks).toEqual([]);
  expect(t.lanes[1]!.name).toBe("09R/27L");
  expect(t.lanes[1]!.marks).toEqual([
    {
      kind: "arrival",
      flight: "BAW1",
      aircraft: "A320",
      end: "27L",
      atMs: 10,
      vacatedMs: 60,
      delayMs: 0,
      emergency: null,
    },
  ]);
});

test("a flight that never waited is never counted as waiting", () => {
  const t = timeline([landed("27L", 10, 0)]);
  expect(t.maxWaiting).toBe(0);
  expect(t.waiting.every((w) => w.waiting >= 0)).toBe(true);
});

test("a closure lasts until the last reason is lifted", () => {
  const t = timeline([
    { type: "runway_closed", runway: "09L/27R", at_ms: 10, reason: "storm" },
    { type: "runway_closed", runway: "09L/27R", at_ms: 20, reason: "maintenance" },
    { type: "runway_opened", runway: "09L/27R", at_ms: 30, reason: "storm", open: false },
    { type: "runway_opened", runway: "09L/27R", at_ms: 40, reason: "maintenance", open: true },
  ]);
  expect(t.lanes[0]!.bands).toEqual([{ kind: "closed", fromMs: 10, toMs: 40, label: "closed: storm" }]);
});

test("a closure still in force runs to the end", () => {
  const t = timeline([{ type: "runway_closed", runway: "09L/27R", at_ms: 10, reason: "storm" }]);
  expect(t.lanes[0]!.bands.map(span)).toEqual([["closed", 10, 100]]);
});

test("a runway unused at the start is shaded from zero", () => {
  const t = timeline([{ type: "active_runways", at_ms: 50, runways: ["09L", "09R"] }], 100, ["09L", null]);
  expect(t.lanes[1]!.bands.map(span)).toEqual([["unused", 0, 50]]);
  expect(t.lanes[0]!.bands).toEqual([]);
});

test('a closed runway shows the closure, not "not in use"', () => {
  const t = timeline(
    [
      { type: "runway_closed", runway: "09R/27L", at_ms: 20, reason: "storm" },
      { type: "runway_opened", runway: "09R/27L", at_ms: 40, reason: "storm", open: true },
    ],
    100,
    ["09L", null],
  );
  expect(t.lanes[1]!.bands.sort(byStart).map(span)).toEqual([
    ["unused", 0, 20],
    ["closed", 20, 40],
    ["unused", 40, 100],
  ]);
});

test("a zero-length band is dropped", () => {
  const t = timeline([
    { type: "runway_closed", runway: "09L/27R", at_ms: 10, reason: "storm" },
    { type: "runway_opened", runway: "09L/27R", at_ms: 10, reason: "storm", open: true },
  ]);
  expect(t.lanes[0]!.bands).toEqual([]);
});

test("ground stops become stop bands", () => {
  const t = timeline(
    [
      { type: "ground_stop_started", at_ms: 10, reason: "storm" },
      { type: "ground_stop_started", at_ms: 15, reason: "curfew" },
      { type: "ground_stop_lifted", at_ms: 20, reason: "storm", clear: false },
      { type: "ground_stop_lifted", at_ms: 30, reason: "curfew", clear: true },
      { type: "ground_stop_started", at_ms: 60, reason: "incident" },
    ],
    100,
  );
  expect(t.stops.map((b) => [b.fromMs, b.toMs, b.label])).toEqual([
    [10, 30, "ground stop: storm"],
    [60, 100, "ground stop: incident"],
  ]);
});

test("an output for an unknown runway is an error", () => {
  expect(() => timeline([landed("99", 10, 0)])).toThrow("unknown runway 99");
});

test("wind notes are padded to three digits", () => {
  const t = timeline([{ type: "wind_changed", at_ms: 0, direction_deg: 5, speed_kt: 10 }]);
  expect(t.notes).toEqual([{ atMs: 0, text: "wind 005°/10 kt" }]);
});
