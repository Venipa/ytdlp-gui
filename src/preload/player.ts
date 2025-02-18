import { SoundSource } from '@main/lib/soundTypes'
import { logger } from '@shared/logger'
import { readFile } from 'fs/promises'
import { GlobalKeyboardListener, IGlobalKey, IGlobalKeyListener } from 'node-global-key-listener'

export interface SoundSetOptions {
  reuseAudioContext: boolean
}
export class SoundSetPlayer {
  #audioContext!: AudioContext
  #gainNode!: GainNode
  #volumeLevel: number = 15
  #globalListener?: GlobalKeyboardListener = null as any
  #soundBuffers: { id: string; press: Record<string, any>; release: Record<string, any> } =
    {} as any
  #initialized: boolean = false
  #reuseAudioContext: boolean = false
  #destroyed: boolean = false
  constructor({ reuseAudioContext }: Partial<SoundSetOptions> = {}) {
    this.#reuseAudioContext = !!reuseAudioContext
    this.initialize()
  }
  get isInitialized() {
    return !!this.#globalListener && this.#initialized && !this.#destroyed
  }
  async loadSound(audioPath: string, soundName: string, type: string) {
    if (!this.#soundBuffers) throw new Error('Failed to load sound.')
    try {
      const data = await readFile(audioPath)
      const arrayBuffer = data.buffer
      const audioBuffer = await this.#audioContext.decodeAudioData(arrayBuffer)
      this.#soundBuffers[type][soundName] = audioBuffer
      logger.info(`${type} sound loaded:`, soundName)
    } catch (error) {
      logger.error('Error loading sound:', error, 'Path:', audioPath)
    }
  }
  async loadSoundSet(soundSet: SoundSource) {
    if (!soundSet) {
      console.error('Failed to load sound set.')
      return
    }
    if (this.#soundBuffers?.id === soundSet.key) return

    this.#soundBuffers = { id: soundSet.key, press: {}, release: {} }

    for (const [key, audioPath] of Object.entries(soundSet.press)) {
      await this.loadSound(audioPath, key, 'press')
    }

    for (const [key, audioPath] of Object.entries(soundSet.release)) {
      await this.loadSound(audioPath, key, 'release')
    }

    logger.info('Sound set loaded:', soundSet.key)
  }

  playSound(soundName: string, type: string) {
    if (this.#destroyed || !this.#initialized) return
    let bufferToPlay
    if (
      ['SPACE', 'ENTER', 'BACKSPACE'].includes(soundName) &&
      this.#soundBuffers[type][soundName]
    ) {
      bufferToPlay = this.#soundBuffers[type][soundName]
    } else {
      const genericKeys = Object.keys(this.#soundBuffers[type]).filter((key) =>
        key.startsWith('GENERIC')
      )
      const randomGenericKey = genericKeys[Math.floor(Math.random() * genericKeys.length)]
      bufferToPlay = this.#soundBuffers[type][randomGenericKey]
    }

    if (this.#audioContext && bufferToPlay) {
      const source = this.#audioContext.createBufferSource()
      source.buffer = bufferToPlay
      source.connect(this.#gainNode)
      source.start(0)
    }
  }
  #echoEnabled: boolean = false
  #oscillator!: OscillatorNode
  setEcho(enabled: boolean = this.#echoEnabled) {
    if (!enabled && this.#oscillator) {
      this.#oscillator.disconnect()
      return
    }
    if (this.#oscillator) return
    this.#echoEnabled = enabled
    const oscillator = (this.#oscillator = this.#audioContext.createOscillator())
    oscillator.type = 'sine'
    oscillator.frequency.setValueAtTime(420, this.#audioContext.currentTime)
    const biquadFilter = this.#audioContext.createBiquadFilter()
    biquadFilter.type = 'lowpass'
    biquadFilter.frequency.setValueAtTime(200, this.#audioContext.currentTime + 1)
    oscillator.connect(biquadFilter)
    logger.debug({ oscillator })
  }
  #distinctKeyPress
  setDistinctKeyPress(value: boolean = false) {
    this.#distinctKeyPress = value
  }
  updateVolume(value: number) {
    this.#volumeLevel = Math.min(Math.max(value, 0), 100)
    if (this.#gainNode) {
      const gainValue = 0.08 * value
      this.#gainNode.gain.setValueAtTime(gainValue, this.#audioContext.currentTime)
    }
    console.log({ volume: this.#volumeLevel })
  }
  /**
   * [string, boolean] = [soundName uppercase, press state (down = true, up = false)]
   */
  #prevState: [string, boolean] = null as any
  #distinctKeyMap: IGlobalKey[] = [
    'LEFT SHIFT',
    'RIGHT SHIFT',
    'SPACE',
    'W',
    'A',
    'S',
    'D',
    'UP ARROW',
    'DOWN ARROW',
    'RIGHT ARROW',
    'LEFT ARROW',
    'LEFT ALT',
    'RIGHT ALT',
    'LEFT CTRL',
    'RIGHT CTRL',
    'LEFT META',
    'RIGHT META',
    'BACKSPACE',
    'RETURN'
  ] as const
  readonly #keyHandler: IGlobalKeyListener = (e, down) => {
    let soundName = e.name?.toUpperCase()
    if (!soundName) return
    if (soundName === 'RETURN') soundName = 'ENTER'
    if (e.name?.startsWith('MOUSE')) return
    const isDown = e.state === 'DOWN'
    let shouldPlaySound = true
    if (this.#distinctKeyPress) {
      shouldPlaySound = false
      const [prevSoundName, prevWasDown] = this.#prevState ?? []
      if (
        down &&
        this.#distinctKeyMap.includes(soundName as any) &&
        (!prevSoundName || (soundName === this.#prevState[0] && isDown !== prevWasDown))
      ) {
        this.#prevState = [soundName, isDown]
        shouldPlaySound = true
      }
    }
    if (down && isDown) {
      this.playSound(soundName, 'press')
    } else {
      this.playSound(soundName, 'release')
    }
  }
  initialize() {
    if (this.isInitialized) throw new Error('Already initialized')
    if (!this.#audioContext || this.#audioContext.state === 'closed') {
      this.#audioContext = new (window.AudioContext ||
        window['webkitAudioContext'])() as AudioContext
      this.#gainNode = this.#audioContext.createGain()
      this.#gainNode.connect(this.#audioContext.destination)
    }
    this.#initialized = true
    this.#destroyed = false
    if (!this.#globalListener) {
      const v = (this.#globalListener = new GlobalKeyboardListener({
        mac: {
          onError: (errorCode) => console.error('ERROR: ' + errorCode)
        }
      }))
      v.addListener(this.#keyHandler)
    }
  }
  destroy() {
    if (!this.#reuseAudioContext) {
      this.#gainNode?.disconnect()
      this.#audioContext?.close()
      this.#audioContext = null as any
      this.#gainNode = null as any
      this.#globalListener?.kill()
      this.#globalListener = null as any
    }
    this.#initialized = false
    this.#destroyed = true
  }
}
