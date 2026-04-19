import { observable } from "@trpc/server/observable";
import { z } from "zod";
import { publicProcedure, router } from "../core/trpc";
import { eventsEmitter } from "./events.ee";
import { EventNameSchema } from "./events.types";

export const eventsRouter = router({
	signal: publicProcedure.input(z.union([z.string(), EventNameSchema])).subscription(({ input: eventName }) => {
		return observable<any>((emit) => {
			function onStatusChange(data: any) {
				emit.next(data);
			}

			eventsEmitter.on(eventName, onStatusChange);

			return () => {
				eventsEmitter.off(eventName, onStatusChange);
			};
		});
	}),
});
