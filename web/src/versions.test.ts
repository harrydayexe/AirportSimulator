// The wasm-bindgen library (Cargo.toml) and the CLI that generates the
// bindings (justfile, CI) must be the same version, or the bindings break.
import { expect, test } from "bun:test";

const root = new URL("../../", import.meta.url);
const read = (path: string) => Bun.file(new URL(path, root)).text();

function version(text: string, pattern: RegExp, file: string): string {
  const found = text.match(pattern)?.[1];
  if (found === undefined) throw new Error(`no wasm-bindgen version in ${file}`);
  return found;
}

test("wasm-bindgen is pinned to one version everywhere", async () => {
  const cargo = version(await read("Cargo.toml"), /^wasm-bindgen = "=([^"]+)"/m, "Cargo.toml");
  const just = version(await read("justfile"), /^wasm_bindgen_version := "([^"]+)"/m, "justfile");
  const ci = version(await read(".github/workflows/ci.yml"), /wasm-bindgen@([\w.]+)/, "ci.yml");
  expect(just).toBe(cargo);
  expect(ci).toBe(cargo);
});
