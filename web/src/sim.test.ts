import { beforeAll, expect, test } from "bun:test";
import * as wasmModule from "./wasm/airport_sim_wasm.js";
import init, { demoScenario, runScenario } from "./wasm/airport_sim_wasm.js";
import type { AirportConfig, RunResult } from "./sim";
import { buildTimeline } from "./timeline";

beforeAll(async () => {
  const bytes = await Bun.file(new URL("./wasm/airport_sim_wasm_bg.wasm", import.meta.url)).arrayBuffer();
  await init({ module_or_path: bytes });
});

/** Runs the native command-line program from the repository root. */
function native(args: string[], stdin?: string): string {
  const run = Bun.spawnSync(["cargo", "run", "--quiet", "--package", "airport-sim", "--", ...args], {
    cwd: new URL("../..", import.meta.url).pathname,
    stdin: stdin === undefined ? "ignore" : new TextEncoder().encode(stdin),
    stderr: "inherit",
  });
  if (run.exitCode !== 0) throw new Error(`airport-sim ${args.join(" ")} failed`);
  return run.stdout.toString().trim();
}

// Run-it-twice, across the two builds: the browser's WebAssembly and the
// native program must agree on every output and on the final fingerprint.
// The last seed is above 2^32 and the largest `u64`: seeds printed by a bulk
// native run must replay in the browser.
test.each(["1", "7", "123456", "18446744073709551615"])(
  "WebAssembly and native agree for seed %s",
  (seed) => {
    const scenario = demoScenario(BigInt(seed), 4, 45, 40);
    expect(native(["gen", "--seed", String(seed), "--hours", "4", "--arrivals", "45", "--departures", "40"])).toBe(
      scenario,
    );

    const wasm = runScenario(scenario);
    expect(runScenario(scenario)).toBe(wasm);
    const fromNative = native(["run", "-", "--json"], scenario);
    expect(JSON.parse(fromNative).end.fingerprint).toBe(JSON.parse(wasm).end.fingerprint);
    expect(fromNative).toBe(wasm);
  },
  300_000,
);

test("runScenario reports a bad scenario", () => {
  expect(() => runScenario("{")).toThrow("invalid scenario");
});

test("runScenario reports an invalid airport", () => {
  const noRunways = JSON.stringify({ airport: { name: "X", runways: [] }, commands: [] });
  expect(() => runScenario(noRunways)).toThrow("invalid airport");
});

// A new export must be a deliberate change.
test("the WebAssembly module exports exactly the documented functions", () => {
  const functions = Object.entries(wasmModule)
    .filter(([, value]) => typeof value === "function")
    .map(([name]) => name)
    .sort();
  expect(functions).toEqual(["default", "demoScenario", "initSync", "runScenario"]);
});

test("the timeline accounts for every movement and closure", () => {
  const scenario = demoScenario(2n, 6, 40, 40);
  const airport = (JSON.parse(scenario) as { airport: AirportConfig }).airport;
  const result = JSON.parse(runScenario(scenario)) as RunResult;
  const timeline = buildTimeline(airport, result);

  expect(timeline.lanes.map((l) => l.name)).toEqual(["09L/27R", "09R/27L", "18/36"]);
  const marks = timeline.lanes.flatMap((l) => l.marks);
  expect(marks.length).toBe(result.end.stats.landed + result.end.stats.departed);
  expect(timeline.diversions.length).toBe(result.end.stats.diverted);

  const [north, south, cross] = timeline.lanes;
  // Maintenance on the southern runway from 20% to 35% of six hours.
  expect(south!.bands.filter((b) => b.kind === "closed")).toEqual([
    { kind: "closed", fromMs: 4_320_000, toMs: 7_560_000, label: "closed: maintenance" },
  ]);
  // The crossing runway is only in use during the gale, 50% to 66%.
  expect(cross!.bands.map((b) => [b.kind, b.fromMs, b.toMs])).toEqual([
    ["unused", 0, 10_800_000],
    ["unused", 14_256_000, timeline.endMs],
  ]);
  expect(north!.bands.some((b) => b.kind === "inspection")).toBe(true);
  // No mark falls inside a band on its own runway.
  for (const lane of timeline.lanes) {
    for (const m of lane.marks) {
      expect(lane.bands.some((b) => m.atMs > b.fromMs && m.atMs < b.toMs)).toBe(false);
    }
  }
  // Everyone has stopped waiting by the end.
  expect(timeline.waiting.at(-1)?.waiting).toBe(0);
  expect(timeline.maxWaiting).toBeGreaterThan(5);
});
