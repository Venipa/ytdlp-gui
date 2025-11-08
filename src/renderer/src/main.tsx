import "non.geist";
import "non.geist/mono";
import "./main.css";

import { Logger } from "@shared/logger";
import { createRoot } from "react-dom/client";
import { Routes } from "./routes";
if (import.meta.env.PROD) Logger.enableProductionMode();
const htmlElement = document.getElementsByTagName("html")![0];
const container = document.getElementById("root")!;
import.meta.env.DEV && htmlElement.classList.add("dark");
if (window.api.useMica) document.body.classList.add("bg-background/80");
else document.body.classList.add("bg-background");
container.classList.add(..."flex flex-col flex-auto h-full".split(" "));

createRoot(container).render(<Routes />);
