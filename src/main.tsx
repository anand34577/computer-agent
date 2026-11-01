import React, { lazy, Suspense, type ComponentType } from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";

const route = window.location.hash.replace("#", "");

if (route.startsWith("vm")) {
  // The hidden Linux VM window has no interface.
  import("./vm/host").then((m) => m.startVm(Number(route.split("=")[1]) || 128));
} else {
  // Each window loads only its own code: the floating bar and pointer stay small.
  const load: () => Promise<{ default: ComponentType }> =
    route === "bar" ? () => import("./Bar")
    : route === "pointer" ? () => import("./Pointer")
    : () => import("./live").then((l) => { l.initLive(); return import("./App"); });
  if (route === "bar") document.body.classList.add("bar-body");
  if (route === "pointer") document.body.classList.add("pointer-body");
  const Root = lazy(load);

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <Suspense fallback={null}>
        <Root />
      </Suspense>
    </React.StrictMode>,
  );
}
