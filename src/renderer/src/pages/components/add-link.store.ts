import { atom, useAtom } from 'jotai'

export const linkStore = atom('')
export const useLinkStore = () => useAtom(linkStore)
