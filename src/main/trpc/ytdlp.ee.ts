import EventEmitter from "events";

export const ytdlpEvents = new EventEmitter()
ytdlpEvents.setMaxListeners(10000)
