import React from "react";
import ReactDOM from "react-dom/client";
import "katex/dist/katex.min.css";
import "./styles.css";
import App from "./App";
import { initLive } from "./live";
import Bar from "./Bar";
import Pointer from "./Pointer";

const route = window.location.hash.replace("#", "");

if (route.startsWith("vm")) {
  // The hidden Linux VM window has no interface.
  import("./vm/host").then((m) => m.startVm(Number(route.split("=")[1]) || 128));
} else {
  const Root = route === "bar" ? Bar : route === "pointer" ? Pointer : App;
  if (route === "bar") document.body.classList.add("bar-body");
  if (route === "pointer") document.body.classList.add("pointer-body");
  if (Root === App) initLive();

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <Root />
    </React.StrictMode>,
  );
}
