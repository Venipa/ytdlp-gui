import { Logger } from "@shared/logger";
import { BrowserWindow, clipboard } from "electron";
const HTTPS = /^https/gi;
const log = new Logger("ClipboardMonitor");
export class ClipboardMonitor {
	constructor(
		private window: BrowserWindow,
		private config: { onHttpsText?: (value: string) => void; distinct?: boolean },
	) {}
	private _listening = false;
	get listening() {
		return this._listening;
	}
	start() {
		if (this._listening) return;
		this._listening = true;
		log.debug("start listening");

		this.window.webContents.on("focus", this.checkClipboard.bind(this));
		this.window.on("restore", this.checkClipboard.bind(this));
		this.window.webContents.on("blur", this.checkClipboard.bind(this));
	}
	stop() {
		if (!this._listening) return;
		this._listening = false;
		this.window.webContents.off("focus", this.checkClipboard.bind(this));
		this.window.off("restore", this.checkClipboard.bind(this));
		this.window.webContents.off("blur", this.checkClipboard.bind(this));
	}
	private _lastText: string | null = null;
	private checkClipboard() {
		const text = clipboard.readText("clipboard");
		if (text) {
			if (HTTPS.test(text) && (!this.config.distinct || !this._lastText || this._lastText !== text)) {
				this.config.onHttpsText?.(text.replace(HTTPS, "https"));
				if (this.config.distinct) this._lastText = text;
				clipboard.clear();
			}
		}
	}
	destroy() {
		this.stop();
	}
}
