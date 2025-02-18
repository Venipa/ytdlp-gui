import { clsx, type ClassValue } from 'clsx'
import merge from 'lodash-es/merge'
import { CSSProperties } from 'react'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
type StyleValue = CSSProperties | string | number | bigint | null | boolean | undefined;
export function sn(...inputs: StyleValue[]): CSSProperties {
  return inputs.reduce((acc, styles) => {
    if (styles && typeof styles === "object") return merge(acc, styles);
    return acc;
  }, {} as CSSProperties) as CSSProperties
}
