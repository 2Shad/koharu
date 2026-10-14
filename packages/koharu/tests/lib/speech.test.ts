import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  castMembers,
  installSpeech,
  playSpeech,
  stopSpeech,
  toggleSpeech,
  useSpeech,
} from '@/lib/speech'
import { commands, type Layer, type SpeechModel } from '@koharu/bridge/protocol'

class FakeAudio extends EventTarget {
  static instances: FakeAudio[] = []
  readonly play = vi.fn(() => Promise.resolve())
  readonly pause = vi.fn()

  constructor(readonly src: string) {
    super()
    FakeAudio.instances.push(this)
  }
}

describe('dialogue playback', () => {
  beforeEach(() => {
    FakeAudio.instances = []
    vi.stubGlobal('Audio', FakeAudio)
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:speech')
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined)
  })

  afterEach(() => {
    stopSpeech()
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('voices a layer, plays it, and clears when the line ends', async () => {
    let resolve: (bytes: number[]) => void = () => undefined
    const speak = vi
      .spyOn(commands, 'speak')
      .mockReturnValue(new Promise((done) => (resolve = done)))

    const playing = playSpeech('layer', true)
    expect(speak).toHaveBeenCalledWith('layer', true)
    expect(useSpeech.getState()).toMatchObject({ layer: 'layer', status: 'loading' })

    resolve([82, 73, 70, 70])
    await playing
    expect(useSpeech.getState()).toMatchObject({ layer: 'layer', status: 'playing' })
    expect(FakeAudio.instances[0]?.play).toHaveBeenCalled()

    FakeAudio.instances[0]?.dispatchEvent(new Event('ended'))
    expect(useSpeech.getState()).toMatchObject({ layer: null, status: null })
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:speech')
  })

  it('stops a line instead of restarting it when toggled again', async () => {
    vi.spyOn(commands, 'speak').mockResolvedValue([0])
    toggleSpeech('layer')
    await vi.waitFor(() => expect(useSpeech.getState().status).toBe('playing'))

    toggleSpeech('layer')
    expect(FakeAudio.instances[0]?.pause).toHaveBeenCalled()
    expect(useSpeech.getState()).toMatchObject({ layer: null, status: null })
  })

  it('ignores a voiced line that arrives after another line started', async () => {
    let resolveFirst: (bytes: number[]) => void = () => undefined
    vi.spyOn(commands, 'speak')
      .mockReturnValueOnce(new Promise((done) => (resolveFirst = done)))
      .mockResolvedValueOnce([0])

    const first = playSpeech('first')
    await playSpeech('second')
    resolveFirst([0])
    await first

    expect(FakeAudio.instances).toHaveLength(1)
    expect(useSpeech.getState()).toMatchObject({ layer: 'second', status: 'playing' })
  })
})

describe('cast members', () => {
  const line = (id: string, character: string | null) =>
    ({
      type: 'text',
      id,
      content: { dialogue: character === null ? null : { script: 'Hi.', character } },
    }) as unknown as Layer

  it('lists configured voices and page speakers once, marking cloned voices', () => {
    const voices = [
      { character: 'Ayane', reference_audio: 'ayane.wav', reference_text: 'Hi.' },
      { character: '  ', reference_audio: 'blank.wav', reference_text: 'Hi.' },
    ]
    expect(
      castMembers(voices, [
        line('a', 'ayane'),
        line('b', 'Serika'),
        line('c', null),
        line('d', ' '),
      ]),
    ).toEqual([
      { name: 'Ayane', voiced: true },
      { name: 'Serika', voiced: false },
    ])
  })
})

describe('voice runtime install', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('runs one install at a time and clears the flag when it settles', async () => {
    let resolve: (model: SpeechModel) => void = () => undefined
    const install = vi
      .spyOn(commands, 'installSpeech')
      .mockReturnValue(new Promise((done) => (resolve = done)))

    const first = installSpeech()
    expect(useSpeech.getState().installing).toBe(true)
    expect(await installSpeech()).toBeNull()
    expect(install).toHaveBeenCalledTimes(1)

    const model = { ready: true, local: false, download_bytes: 1 }
    resolve(model)
    expect(await first).toEqual(model)
    expect(useSpeech.getState().installing).toBe(false)
  })
})
