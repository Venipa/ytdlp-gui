import { clamp } from 'lodash-es'
import { isProduction } from '../config'

/**
 * (development env)
 * Promise to wait x ms, simulates huge dataset processing.
 * @param timeMs delay in ms
 * @returns
 */
export const wait = (timeMs: number) => {
  if (isProduction) return Promise.resolve()
  return new Promise<void>((resolve) =>
    setTimeout(() => {
      resolve()
    }, timeMs)
  )
}
/**
 * Promise to wait for next tick
 * @returns
 */
export const nextTick = () => {
  return new Promise<void>((resolve) =>
    setTimeout(() => {
      resolve()
    })
  )
}
export const throwNull = <T, R = Exclude<T, null | undefined>>(data: T) => {
  if (!data) throw new Error('Item(s) not found')
  return data as R
}

export const defaultNull = <T, R = Exclude<T, undefined>>(data: T) => {
  if (!data) data = null as T;
  return data as R | null
}

export type TaskPromise<T> = () => Promise<T>;
export default async function queuePromise<T = any>(promises: Array<TaskPromise<T>>) {
  const data: T[] = [];
  return await promises.reduce(async (acc, item) => {
    return await acc.then(
      async (data) =>
        await item().then((v) => {
          data.push(v);
          return data;
        })
    );
  }, Promise.resolve(<typeof data>[]));
}
export async function queuePromiseStack<T = any>(promises: Array<TaskPromise<T>>, stackSize: number = 100) {
  const queueStack = chunk(promises, stackSize).map((promiseChunk) => async () => await Promise.all(promiseChunk.map(async (p) => await p())));
  return await queuePromise(queueStack).then((data) =>
    data.reduce<T[]>((acc, r) => {
      if (r?.length) acc.push(...r);
      return acc;
    }, [])
  );
}
export async function queuePromiseAutoStack<T = any>(promises: Parameters<typeof queuePromiseStack<T>>[0], maxPerStack = 90) {
  const stackRes = promises.length / maxPerStack;
  const stackSize = promises.length < maxPerStack ? 10 : clamp(Math.ceil(stackRes * maxPerStack), 1, maxPerStack); // max per stack fallback
  return await queuePromiseStack<T>(promises, stackSize);
}

export function chunk<T>(arr: T[], size: number) {
  return Array.from({ length: Math.ceil(arr.length / size) }, (_v, i) => arr.slice(i * size, i * size + size));
}
