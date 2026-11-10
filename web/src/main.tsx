import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { gapMs, loadSim } from "./sim";

function App() {
  const [gap, setGap] = useState<number | undefined>();
  const [error, setError] = useState<string | undefined>();

  useEffect(() => {
    loadSim()
      .then(() => setGap(gapMs(5, 140)))
      .catch((e: unknown) => setError(String(e)));
  }, []);

  return (
    <main>
      <h1>Airport Simulator</h1>
      {error && <p role="alert">Failed to load simulator: {error}</p>}
      {gap !== undefined && (
        <p>
          5 nm at 140 kt, computed in Rust: <output>{gap}</output> ms
        </p>
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
