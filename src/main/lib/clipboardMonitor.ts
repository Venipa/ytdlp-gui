import { clipboard } from 'electron';
const HTTPS = /^https/gi
export class ClipboardMonitor {
  constructor(private config: { onHttpsText?: (value: string) => void; distinct?: boolean }) {}
  private _handle: NodeJS.Timeout
  start() {
    if (!this._handle) this.checkClipboard()
  }
  stop() {
    this.destroy()
  }
  private _lastText: string | null = null
  private checkClipboard() {
    const text = clipboard.readText('clipboard')
    if (text) {
      if (
        HTTPS.test(text) &&
        (!this.config.distinct || !this._lastText || this._lastText !== text)
      ) {
        this.config.onHttpsText?.(text.replace(HTTPS, 'https'))
        this._lastText = text
      }
    }
    if (this._handle) clearTimeout(this._handle)
    this._handle = setTimeout(this.checkClipboard.bind(this), 2500)
  }
  destroy() {
    if (this._handle) clearTimeout(this._handle)
  }
}
