import { ZodEffects, ZodNullable, ZodOptional, ZodType, ZodTypeAny } from 'zod';

type IsNullable<T> = Extract<T, null> extends never ? false : true;
type IsOptional<T> = Extract<T, undefined> extends never ? false : true;

type ZodWithEffects<T extends ZodTypeAny> = T | ZodEffects<T, unknown, unknown>;

export type ToZodSchema<T extends Record<string, any>> = {
  [K in keyof T]-?: IsNullable<T[K]> extends true
    ? ZodWithEffects<ZodNullable<ZodType<T[K]>>>
    : IsOptional<T[K]> extends true
      ? ZodWithEffects<ZodOptional<ZodType<T[K]>>>
      : ZodWithEffects<ZodType<T[K]>>;
};
