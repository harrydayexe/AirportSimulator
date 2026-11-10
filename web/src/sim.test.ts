import { beforeAll, expect, test } from "bun:test";
import init, { gapMs } from "./wasm/airport_sim_wasm.js";

beforeAll(async () => {
  const bytes = await Bun.file(new URL("./wasm/airport_sim_wasm_bg.wasm", import.meta.url)).arrayBuffer();
  await init({ module_or_path: bytes });
});

// Must match the native result in crates/airport (run-it-twice in miniature).
test("gapMs matches the native Rust result", () => {
  expect(gapMs(5, 140)).toBe(128_571);
});

test("gapMs is undefined for zero speed", () => {
  expect(gapMs(5, 0)).toBeUndefined();
});
