'use client'

import { useEffect, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { ActivityBody } from '@/components/editor/ActivityCenter'
import { Inspector } from '@/components/editor/Inspector'
import { PageRail } from '@/components/editor/PageRail'
import { registerBackHandler } from '@/lib/back'
import { useKoharuStore, type MobilePanel } from '@/lib/store'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@koharu/ui/components/sheet'

// On mobile the side panels are bottom sheets over the canvas. Only one is open at a
// time; the app bar and the Android back button both dismiss it.
export function MobileSheets() {
  const { t } = useTranslation()
  const panel = useKoharuStore((state) => state.mobilePanel)
  const setPanel = useKoharuStore((state) => state.setMobilePanel)

  useEffect(() => {
    if (panel === null) return
    return registerBackHandler(() => {
      setPanel(null)
      return true
    })
  }, [panel, setPanel])

  return (
    <>
      <MobileSheet panel='pages' title={t('navigator.pages')}>
        <PageRail />
      </MobileSheet>
      <MobileSheet panel='inspector' title={t('agent.properties')}>
        <Inspector />
      </MobileSheet>
      <MobileSheet panel='activity' title={t('activity.title')}>
        <ActivityBody />
      </MobileSheet>
    </>
  )
}

function MobileSheet({
  panel,
  title,
  children,
}: {
  panel: MobilePanel
  title: string
  children: ReactNode
}) {
  const active = useKoharuStore((state) => state.mobilePanel)
  const setPanel = useKoharuStore((state) => state.setMobilePanel)

  return (
    <Sheet open={active === panel} onOpenChange={(open) => setPanel(open ? panel : null)}>
      <SheetContent
        side='bottom'
        className='h-[76dvh] max-h-[76dvh] gap-0 overflow-hidden pb-[env(safe-area-inset-bottom)]'
      >
        <SheetHeader className='shrink-0 border-b border-border/60 pr-12'>
          <SheetTitle>{title}</SheetTitle>
        </SheetHeader>
        <div className='flex min-h-0 flex-1 flex-col overflow-y-auto'>{children}</div>
      </SheetContent>
    </Sheet>
  )
}
