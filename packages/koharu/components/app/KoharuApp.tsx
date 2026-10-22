'use client'

import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { TitleBar } from '@/components/app/TitleBar'
import { Editor } from '@/components/editor/Editor'
import { SettingsPage } from '@/components/preferences/SettingsPage'
import { StartView } from '@/components/start/StartView'
import { startBackButton } from '@/lib/back'
import { useProject } from '@/lib/queries'
import { useKoharuStore } from '@/lib/store'
import { cn } from '@koharu/ui/lib/utils'

export function KoharuApp() {
  const { t } = useTranslation()
  const project = useProject().data
  const selectedPages = useKoharuStore((state) => state.selectedPages)
  const selectPages = useKoharuStore((state) => state.selectPages)
  const selectLayers = useKoharuStore((state) => state.selectLayers)
  const projectLoaded = project !== undefined
  const settingsOpen = useKoharuStore((state) => state.settingsOpen)
  const platform = useKoharuStore((state) => state.platform)
  const activePage = project?.active_page
  const editorOpen = project !== undefined && project !== null && !settingsOpen

  useEffect(() => {
    if (!projectLoaded || selectedPages.length > 0) return
    selectPages(activePage ? [activePage] : [])
    selectLayers([])
  }, [activePage, projectLoaded, selectLayers, selectPages, selectedPages.length])

  // Android hands every back press to the registered handler. Surfaces register
  // through lib/back; when none consumes the press the app backgrounds through the
  // typed command instead of leaving the webview's own history in charge.
  useEffect(() => {
    if (platform !== 'mobile') return
    return startBackButton()
  }, [platform])

  return (
    <div
      className={cn(
        'relative flex h-dvh w-dvw flex-col overflow-hidden pl-[env(safe-area-inset-left)] pr-[env(safe-area-inset-right)] pt-[env(safe-area-inset-top)] pb-[env(safe-area-inset-bottom)] text-foreground',
        editorOpen ? 'bg-transparent' : 'bg-[var(--surface-titlebar)]',
      )}
    >
      {platform === 'desktop' && <TitleBar />}
      {settingsOpen ? (
        <SettingsPage />
      ) : project === undefined ? (
        <main className='grid min-h-0 flex-1 place-items-center bg-[var(--surface-canvas)]'>
          <div className='flex items-center gap-3 text-[12px] text-muted-foreground'>
            <span className='size-2 rounded-full bg-primary' />
            {t('startup.opening')}
          </div>
        </main>
      ) : project === null ? (
        <StartView />
      ) : (
        <Editor />
      )}
    </div>
  )
}
