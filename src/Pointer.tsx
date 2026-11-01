import { useEffect, useState } from "react";
import { on } from "./api";

/** The agent's own cursor: an arrow with a label that glides to each target and ripples on click. */
export default function Pointer() {
  const [n, setN] = useState(0);
  const [label, setLabel] = useState("");
  const [idle, setIdle] = useState(false);
  useEffect(() => {
    const offs = [
      on<{ label: string }>("pointer-move", (p) => {
        setLabel(p.label);
        setIdle(false);
      }),
      on("pointer-pulse", () => setN((x) => x + 1)),
      on("pointer-idle", () => setIdle(true)),
    ];
    return () => offs.forEach((f) => f());
  }, []);
  return (
    <div className={"pointer" + (idle ? " idle" : "")}>
      <div key={n} className={"ring" + (n ? " go" : "")} />
      <svg className="arrow" width="22" height="26" viewBox="0 0 22 26" aria-hidden="true">
        <path d="M2 2 L2 21 L7.2 16.4 L10.6 24 L14 22.5 L10.7 15.2 L18 15 Z" />
      </svg>
      {label && <div className="pointer-label">{label}</div>}
    </div>
  );
}
