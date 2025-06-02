import clip from "clipboard-event";
import { clipboard } from "electron";
const HTTPS = /^https/gi;
export class ClipboardMonitor {
	constructor(private config: { onHttpsText?: (value: string) => void; distinct?: boolean }) {
		clip.startListening();
	}
	start() {
		clip.on("change", this.checkClipboard.bind(this));
	}
	stop() {
		clip.off("change", this.checkClipboard.bind(this));
	}
	private _lastText: string | null = null;
	private checkClipboard() {
		console.log("clipboard changed");
		const text = clipboard.readText("clipboard");
		if (text) {
			if (HTTPS.test(text) && (!this.config.distinct || !this._lastText || this._lastText !== text)) {
				this.config.onHttpsText?.(text.replace(HTTPS, "https"));
				if (this.config.distinct) this._lastText = text;
			}
		}
	}
	destroy() {
		clip.stopListening();
	}
}
