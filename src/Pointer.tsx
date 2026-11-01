import { useEffect, useState } from "react";
import { on } from "./api";

/** The agent's visible pointer: a ring that pulses just before each click. */
export default function Pointer() {
  const [n, setN] = useState(0);
  useEffect(() => on("pointer-pulse", () => setN((x) => x + 1)), []);
  return (
    <div className="pointer">
      <div key={n} className={"ring" + (n ? " go" : "")} />
    </div>
  );
}
