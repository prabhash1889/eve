import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/figtree/400.css";
import "@fontsource/figtree/500.css";
import "@fontsource/figtree/600.css";
import "@fontsource/fraunces/500.css";
import "@fontsource/fraunces/600.css";
import "./styles/globals.css";
import { Hub } from "./Hub";

// Respect system theme on first paint (a manual toggle lives in the Hub).
if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
  document.documentElement.classList.add("dark");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Hub />
  </React.StrictMode>,
);
