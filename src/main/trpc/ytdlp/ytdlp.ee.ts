import EventEmitter from "events";
export const ytdlpEvents = new EventEmitter();

export const pushToastToClient = (message: string, type?: string, description?: string) => {
	return ytdlpEvents.emit("toast", { message, type, description });
};
ytdlpEvents.setMaxListeners(10000);
export {};
