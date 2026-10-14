'use client'

import { create } from 'zustand'

import {
  commands,
  type CharacterVoice,
  type EntityId,
  type Layer,
  type SpeechModel,
} from '@koharu/bridge/protocol'

import { call } from './backend'

interface SpeechState {
  layer: EntityId | null
  status: 'loading' | 'playing' | null
  /** The voice runtime download outlives the settings page that started it. */
  installing: boolean
}

/** One line plays at a time; starting another stops the current one. */
export const useSpeech = create<SpeechState>(() => ({
  layer: null,
  status: null,
  installing: false,
}))

let audio: HTMLAudioElement | null = null
let source: string | null = null
let generation = 0

export async function playSpeech(layer: EntityId, regenerate = false): Promise<void> {
  stopSpeech()
  const current = generation
  useSpeech.setState({ layer, status: 'loading' })
  try {
    const bytes = new Uint8Array(await call(commands.speak, layer, regenerate))
    if (current !== generation) return
    source = URL.createObjectURL(new Blob([bytes.buffer], { type: 'audio/wav' }))
    audio = new Audio(source)
    const finish = () => {
      if (current === generation) stopSpeech()
    }
    audio.addEventListener('ended', finish)
    audio.addEventListener('error', finish)
    useSpeech.setState({ layer, status: 'playing' })
    await audio.play()
  } catch {
    if (current === generation) stopSpeech()
  }
}

export function stopSpeech(): void {
  generation += 1
  audio?.pause()
  audio = null
  if (source) URL.revokeObjectURL(source)
  source = null
  useSpeech.setState({ layer: null, status: null })
}

export function toggleSpeech(layer: EntityId): void {
  const { layer: current } = useSpeech.getState()
  if (current === layer) stopSpeech()
  else void playSpeech(layer)
}

export async function installSpeech(): Promise<SpeechModel | null> {
  if (useSpeech.getState().installing) return null
  useSpeech.setState({ installing: true })
  try {
    return await call(commands.installSpeech)
  } finally {
    useSpeech.setState({ installing: false })
  }
}

export interface CastMember {
  name: string
  /** Has a reference recording, so lines are voice-cloned. */
  voiced: boolean
}

/** Configured voices plus page speakers, deduplicated case-insensitively like the speech runtime. */
export function castMembers(voices: CharacterVoice[], layers: Layer[]): CastMember[] {
  const cast = new Map<string, CastMember>()
  for (const voice of voices) {
    const name = voice.character.trim()
    if (name) cast.set(name.toLowerCase(), { name, voiced: true })
  }
  for (const layer of layers) {
    const name = layer.type === 'text' ? layer.content.dialogue?.character?.trim() : undefined
    if (name && !cast.has(name.toLowerCase())) cast.set(name.toLowerCase(), { name, voiced: false })
  }
  return [...cast.values()].sort((left, right) => left.name.localeCompare(right.name))
}
