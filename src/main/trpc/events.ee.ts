import EventEmitter from "events";
import { EventNames } from "./events.types";

export const eventsEmitter = new EventEmitter();
export const pushMessageToClient = (...args: any[]) => eventsEmitter.emit("message", ...args);
export const pushChannelToClient = (channel: EventNames, ...args: any[]) => eventsEmitter.emit(channel, ...args);
export type ServerLogType = "info" | "warn" | "error" | "success" | "debug";
export const pushLogToClient = (message: string, type: ServerLogType = "info", ...args: any[]) =>
	pushChannelToClient("log", { date: new Date().toISOString(), message, type, args });
