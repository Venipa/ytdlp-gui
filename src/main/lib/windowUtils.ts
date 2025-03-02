import { createYmlStore } from "@shared/electron/store/createYmlStore";
import { BrowserWindow, screen } from "electron";

export async function wrapWindowHandler(
  win: BrowserWindow,
  windowName: string,
  { width: defaultWidth, height: defaultHeight }: { width: number; height: number },
) {
  const key = "window-state";
  const name = `window-state-${windowName}`;
  const store = createYmlStore(name);
  const defaultSize = {
    width: defaultWidth,
    height: defaultHeight,
  };
  let state: { width: number; height: number; x: number; y: number; maximized?: boolean } | null = null;
  const restore = () => store.get(key, defaultSize);

  const getCurrentPosition = () => {
    const [x, y] = win.getPosition();
    const [width, height] = win.getSize();
    return {
      x,
      y,
      width,
      height,
      maximized: win.isMaximized(),
    };
  };

  const windowWithinBounds = (windowState, bounds) => {
    return (
      windowState.x >= bounds.x &&
      windowState.y >= bounds.y &&
      windowState.x + windowState.width <= bounds.x + bounds.width &&
      windowState.y + windowState.height <= bounds.y + bounds.height
    );
  };

  const resetToDefaults = () => {
    const bounds = screen.getPrimaryDisplay().bounds;
    return Object.assign({}, defaultSize, {
      x: (bounds.width - defaultSize.width) / 2,
      y: (bounds.height - defaultSize.height) / 2,
    });
  };

  const ensureVisibleOnSomeDisplay = (windowState) => {
    const visible = screen.getAllDisplays().some((display) => {
      return windowWithinBounds(windowState, display.bounds);
    });
    if (!visible) {
      // Window is partially or fully not visible now.
      // Reset it to safe defaults.
      return resetToDefaults();
    }
    return windowState;
  };
  const saveState = () => {
    if (!win.isMinimized() && !win.isMaximized()) {
      state = Object.assign({}, state, getCurrentPosition());
    }
    store.set(key, state);
  };
  state = ensureVisibleOnSomeDisplay(restore());
  win.on("close", saveState);
  return { state, saveState };
}
