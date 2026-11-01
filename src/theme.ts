import type { Settings } from "./api";

export function applyTheme(s: Pick<Settings, "theme" | "accent" | "text_size" | "density"> & Partial<Pick<Settings, "radius" | "app_name">>) {
  const root = document.documentElement;
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const theme = s.theme === "system" ? (dark ? "dark" : "light") : s.theme;
  root.dataset.theme = theme;
  root.dataset.density = s.density;
  root.dataset.radius = s.radius ?? "rounded";
  if (theme !== "contrast") root.style.setProperty("--accent", s.accent || "#4f6bed");
  else root.style.removeProperty("--accent");
  root.style.setProperty("--fs", `${s.text_size || 14}px`);
  if (s.app_name) document.title = s.app_name;
}

export const ACCENTS = ["#4f6bed", "#7c5cff", "#0f9d8a", "#16a34a", "#e5484d", "#f97316", "#d946ef", "#0ea5e9", "#475569"];
