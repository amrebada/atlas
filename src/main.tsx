import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import PilotApp from "./features/pilot/PilotApp";
import "./ui/global.css";
import { installDisableOsPredictions } from "./ui/disable-os-predictions";

installDisableOsPredictions();

// The dedicated "pilot" window loads the same bundle; branch on the label.
let isPilotWindow = false;
try {
  isPilotWindow = getCurrentWindow().label === "pilot";
} catch {
  isPilotWindow = false;
}

const Root = isPilotWindow ? PilotApp : App;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
