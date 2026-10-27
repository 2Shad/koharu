'use client'

import { showCanvasPage } from '@koharu/bridge/canvas'
import {
  commands,
  type CanvasPagePreparation,
  type Page,
  type ProjectInfo,
} from '@koharu/bridge/protocol'

import { call } from './backend'
import { pageKey, preparedPageKey, projectKey, queryClient } from './queries'
import { useKoharuStore } from './store'

let selectionRequest = 0

// A closed project or a newer selection must not let an older native reply replace the
// active page cache.
export function cancelPageSelection(): void {
  selectionRequest += 1
}

export function activatePage(pageId: string, next: string[]): void {
  const previousProject = queryClient.getQueryData<ProjectInfo | null>(projectKey)
  const previousPage = queryClient.getQueryData<Page | null>(pageKey)
  const prepared = queryClient.getQueryData<CanvasPagePreparation>(preparedPageKey(pageId))
  const activated = showCanvasPage(pageId, previousProject?.revision ?? null)
  const request = ++selectionRequest
  const synchronize = () => {
    if (selectionRequest !== request) return
    if (activated && previousProject && prepared?.revision === previousProject.revision) {
      queryClient.setQueryData(projectKey, { ...previousProject, active_page: pageId })
      queryClient.setQueryData(pageKey, prepared.page)
    }
    useKoharuStore.getState().selectPages(next)
    useKoharuStore.getState().selectLayers([])
    void call(commands.selectPage, pageId)
      .then((selection) => {
        if (selectionRequest !== request) return
        queryClient.setQueryData(projectKey, selection.project)
        queryClient.setQueryData(pageKey, selection.page)
      })
      .catch(() => {
        if (selectionRequest !== request) return
        if (queryClient.getQueryData<ProjectInfo | null>(projectKey)?.active_page === pageId) {
          queryClient.setQueryData(projectKey, previousProject)
          queryClient.setQueryData(pageKey, previousPage)
        }
      })
  }
  if (activated) {
    requestAnimationFrame(() => window.setTimeout(synchronize, 0))
  } else {
    synchronize()
  }
}
