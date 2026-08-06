import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { SelectionProvider } from "./contexts/SelectionContext";
import "./i18n";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <SelectionProvider>
      <App />
    </SelectionProvider>
  </React.StrictMode>,
);
