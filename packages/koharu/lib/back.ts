'use client'

import { onBackButtonPress } from '@tauri-apps/api/app'

import { commands } from '@koharu/bridge/protocol'

import { dispatch } from './backend'

// Dismissible surfaces register while they are open. The most recent registration
// is consulted first so a nested surface closes before the one that opened it.
const handlers: Array<() => boolean> = []
let listener: Promise<{ unregister: () => void }> | null = null

export function registerBackHandler(handler: () => boolean): () => void {
  handlers.push(handler)
  return () => {
    const index = handlers.lastIndexOf(handler)
    if (index !== -1) handlers.splice(index, 1)
  }
}

// Owns the single Android back-button listener; a second call reuses it. When no
// registered surface consumes the press the app backgrounds through the typed command.
export function startBackButton(): () => void {
  listener ??= onBackButtonPress(() => {
    for (let index = handlers.length - 1; index >= 0; index -= 1) {
      if (handlers[index]()) return
    }
    dispatch(commands.backgroundApp)
  })
  const current = listener
  return () => {
    if (listener !== current) return
    listener = null
    void current.then((registered) => registered.unregister()).catch(() => undefined)
  }
}
