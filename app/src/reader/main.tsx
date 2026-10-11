import React from "react";
import ReactDOM from "react-dom/client";
import { ReaderToolbar } from "./ReaderToolbar";
import "../styles/theme.css";
import "../styles/reader.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ReaderToolbar />
  </React.StrictMode>,
);
