import { StrictMode, useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { type DemoOptions, type DemoRun, loadSim, runDemo } from "./sim";
import { TimelineView } from "./RunwayView";
import { buildTimeline, clock } from "./timeline";

const DEFAULTS: DemoOptions = { seed: 1n, hours: 6, arrivalsPerHour: 40, departuresPerHour: 40 };
const MAX_SEED = 2n ** 64n - 1n;

/** The seed typed in, or `undefined` if it is not a whole number from 0 to 2^64 - 1. */
function parseSeed(text: string): bigint | undefined {
  if (!/^\d{1,20}$/.test(text)) return undefined;
  const seed = BigInt(text);
  return seed <= MAX_SEED ? seed : undefined;
}

type NumberField = Exclude<keyof DemoOptions, "seed">;
const FIELDS: { key: NumberField; label: string; min: number; max: number }[] = [
  { key: "hours", label: "Hours of traffic", min: 1, max: 24 },
  { key: "arrivalsPerHour", label: "Arrivals an hour", min: 0, max: 120 },
  { key: "departuresPerHour", label: "Departures an hour", min: 0, max: 120 },
];

function App() {
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | undefined>();
  const [form, setForm] = useState(DEFAULTS);
  const [seedText, setSeedText] = useState(String(DEFAULTS.seed));
  const seed = parseSeed(seedText);
  const [run, setRun] = useState<DemoRun | undefined>();

  const start = (options: DemoOptions) => {
    try {
      setRun(runDemo(options));
      setError(undefined);
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  useEffect(() => {
    loadSim()
      .then(() => {
        setReady(true);
        start(DEFAULTS);
      })
      .catch((e: unknown) => setError(`Failed to load simulator: ${String(e)}`));
  }, []);

  const timeline = useMemo(() => run && buildTimeline(run.airport, run.result), [run]);
  const stats = run?.result.end.stats;
  const mean = (total: number, count: number) => (count ? `${Math.round(total / count / 1000)} s` : "n/a");

  return (
    <main>
      <header>
        <h1>Airport Simulator</h1>
        <p>
          One airport, simulated in Rust inside this page. Seeded traffic meets a maintenance closure, a gale that
          leaves only the short crossing runway, and a mayday. The same seed always gives the same run.
        </p>
      </header>

      <form
        className="controls"
        onSubmit={(e) => {
          e.preventDefault();
          if (seed !== undefined) start({ ...form, seed });
        }}
      >
        <label>
          Seed
          <input
            type="text"
            inputMode="numeric"
            pattern="\d{1,20}"
            required
            aria-invalid={seed === undefined}
            title="A whole number from 0 to 18446744073709551615"
            value={seedText}
            onChange={(e) => setSeedText(e.target.value.trim())}
          />
        </label>
        {FIELDS.map(({ key, label, min, max }) => (
          <label key={key}>
            {label}
            <input
              type="number"
              min={min}
              max={max}
              required
              value={form[key]}
              onChange={(e) => setForm({ ...form, [key]: Math.min(max, Math.max(min, Math.trunc(Number(e.target.value)))) })}
            />
          </label>
        ))}
        <button type="submit" disabled={!ready || seed === undefined}>
          Run
        </button>
      </form>

      {error && <p role="alert">{error}</p>}

      {run && stats && timeline && (
        <>
          <dl className="stats">
            <div>
              <dt>Landed</dt>
              <dd>{stats.landed}</dd>
              <dd className="sub">mean delay {mean(stats.arrival_delay_ms, stats.landed)}</dd>
            </div>
            <div>
              <dt>Departed</dt>
              <dd>{stats.departed}</dd>
              <dd className="sub">mean delay {mean(stats.departure_delay_ms, stats.departed)}</dd>
            </div>
            <div>
              <dt>Diverted</dt>
              <dd>{stats.diverted}</dd>
              <dd className="sub">held over {Math.round((run.airport.max_arrival_hold_ms ?? 0) / 60_000)} min</dd>
            </div>
            <div>
              <dt>Most waiting at once</dt>
              <dd>{timeline.maxWaiting}</dd>
              <dd className="sub">run ends {clock(timeline.endMs)}</dd>
            </div>
            <div>
              <dt>State fingerprint</dt>
              <dd className="mono" data-testid="fingerprint">
                {run.result.end.fingerprint}
              </dd>
              <dd className="sub">matches the native program for this seed</dd>
            </div>
          </dl>

          <h2>{run.airport.name}: runways over time</h2>
          <TimelineView timeline={timeline} />

          <details>
            <summary>Every movement as a table</summary>
            <div className="table-scroll">
              <table>
                <thead>
                  <tr>
                    <th>Time</th>
                    <th>Flight</th>
                    <th>Type</th>
                    <th>Movement</th>
                    <th>Runway</th>
                    <th className="num">Delay (s)</th>
                    <th>Emergency</th>
                  </tr>
                </thead>
                <tbody>
                  {timeline.lanes
                    .flatMap((l) => l.marks)
                    .sort((a, b) => a.atMs - b.atMs)
                    .map((m) => (
                      <tr key={m.flight}>
                        <td>{clock(m.atMs)}</td>
                        <td>{m.flight}</td>
                        <td>{m.aircraft}</td>
                        <td>{m.kind === "arrival" ? "Landing" : "Take-off"}</td>
                        <td>{m.end}</td>
                        <td className="num">{Math.round(m.delayMs / 1000)}</td>
                        <td>{m.emergency?.replace("_", "-") ?? ""}</td>
                      </tr>
                    ))}
                </tbody>
              </table>
            </div>
          </details>
        </>
      )}
    </main>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
