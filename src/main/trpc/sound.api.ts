import type { SoundSource } from '@main/lib/soundTypes'
import { soundSets, soundStore, SoundStoreSource } from '@main/stores/sound.store'
import { publicProcedure, router } from '@main/trpc/trpc'
import { createLogger } from '@shared/logger'
import { observable } from '@trpc/server/observable'
import { pick } from 'lodash-es'
import { z } from 'zod'
const log = createLogger('soundRouter')
log.debug('init')
const soundConfigKeys = ['echo', 'volume', 'enabled', 'key', 'distinct'] as const
export const soundRouter = router({
  setSoundSet: publicProcedure
    .input(
      z.string().refine((d) => d && soundSets.find((s) => s.key === d), 'Invalid sound set id.')
    )
    .mutation(({ input: soundId }) => {
      if (soundStore.store.active)
        soundStore.store.configs[soundStore.store.active.key] = pick(
          soundStore.store.active,
          ...soundConfigKeys
        )
      const prevConfig = pick(
        soundStore.store.configs[soundId] ?? { echo: false, volume: 15 },
        ...soundConfigKeys
      )
      soundStore.set('active', {
        ...prevConfig,
        ...soundSets.find((d) => d.key === soundId),
        id: soundId
      } as SoundSource)
    }),
  getSoundSet: publicProcedure.input(z.string()).query(({ input: soundId }) => {
    return soundSets.find((d) => d.key === soundId)
  }),
  setVolume: publicProcedure
    .input(
      z
        .number()
        .refine(() => !!soundStore.store.active, 'Missing sound set or not selected')
        .transform((s) => Math.min(Math.max(s, 0), 100))
    )
    .mutation(({ input: volume }) => {
      soundStore.set('active.volume', volume)
      return volume
    }),
  setEnabled: publicProcedure
    .input(z.boolean().default(false).refine(() => !!soundStore.store.active, 'Missing sound set or not selected'))
    .mutation(({ input: enabled }) => {
      soundStore.set('active.enabled', enabled)
      return enabled
    }),
  getActiveSoundSet: publicProcedure.query(() => {
    return soundStore.store.active
  }),
  getAllSoundSets: publicProcedure
    .input(
      z
        .string()
        .nullish()
        .transform((s) => s && s.toLowerCase())
    )
    .query(({ input: q }) => {
      if (q)
        return soundSets.filter((s) =>
          [s.caption, s.key].some((d) => d.toLowerCase().indexOf(q) !== -1)
        )
      return soundSets
    }),
  onSoundSetChange: publicProcedure.subscription(() => {
    return observable<SoundStoreSource, SoundStoreSource>((emit) => {
      const unsubChangeObserver = soundStore.onDidChange('active', (s) =>
        emit.next(s as SoundStoreSource)
      )
      return () => {
        unsubChangeObserver()
      }
    })
  })
})
