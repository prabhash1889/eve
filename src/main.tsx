import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/figtree/400.css";
import "@fontsource/figtree/500.css";
import "@fontsource/figtree/600.css";
import "@fontsource/fraunces/500.css";
import "@fontsource/fraunces/600.css";
import "./styles/globals.css";
import { Hub } from "./Hub";
import { initTheme } from "./lib/theme";

// Apply the saved theme on first paint (the picker lives in the Hub).
initTheme();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Hub />
  </React.StrictMode>,
);
