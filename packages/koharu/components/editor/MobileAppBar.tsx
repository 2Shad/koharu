'use client'

import {
  Activity,
  Download,
  FilePlus2,
  FolderOpen,
  Images,
  LoaderCircle,
  MoreVertical,
  Redo2,
  Settings,
  Share2,
  SlidersHorizontal,
  Undo2,
  X,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { useActivityCount } from '@/components/editor/ActivityCenter'
import { call } from '@/lib/backend'
import {
  pageKey,
  pagesKey,
  projectKey,
  refresh,
  useCommand,
  useImportPages,
  usePage,
  usePages,
  useProject,
} from '@/lib/queries'
import { useKoharuStore } from '@/lib/store'
import { commands, type ExportFormat } from '@koharu/bridge/protocol'
import { Button } from '@koharu/ui/components/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@koharu/ui/components/dropdown-menu'

const exportFormats = ['png', 'psd', 'cbz'] as const satisfies readonly ExportFormat[]

// Touch app bar: navigation to the bottom sheets plus the project actions that the desktop
// title bar carries. Accelerator labels are desktop-only, so none appear here.
export function MobileAppBar() {
  const { t } = useTranslation()
  const project = useProject().data
  const page = usePage().data ?? null
  const pages = usePages(Boolean(project)).data ?? []
  const activePanel = useKoharuStore((state) => state.mobilePanel)
  const setMobilePanel = useKoharuStore((state) => state.setMobilePanel)
  const setSettingsOpen = useKoharuStore((state) => state.setSettingsOpen)
  const activityCount = useActivityCount()
  const { importPages, importing } = useImportPages()
  const { run: exportProject, busy: exporting } = useCommand(
    ['export-project'],
    commands.export,
    'menu.export',
  )

  const togglePanel = (panel: typeof activePanel) =>
    setMobilePanel(activePanel === panel ? null : panel)

  const closeProject = () => void call(commands.closeProject).catch(() => undefined)
  const undo = () =>
    void call(commands.undo)
      .then(() => refresh(projectKey, pagesKey, pageKey))
      .catch(() => undefined)
  const redo = () =>
    void call(commands.redo)
      .then(() => refresh(projectKey, pagesKey, pageKey))
      .catch(() => undefined)
  const share = () => {
    if (!page) return
    void call(commands.share, page.id).catch(() => undefined)
  }

  return (
    <header className='flex h-11 shrink-0 items-center gap-0.5 border-b border-border/80 bg-[var(--surface-titlebar)] px-1 text-[12px]'>
      <div className='min-w-0 flex-1 px-1.5'>
        {project ? (
          <p className='truncate leading-tight'>
            <span className='font-medium text-foreground'>{project.name}</span>
            {page && <span className='text-muted-foreground'> / {page.label}</span>}
          </p>
        ) : (
          <span className='text-muted-foreground'>Koharu</span>
        )}
      </div>

      <Button
        variant={activePanel === 'pages' ? 'secondary' : 'ghost'}
        size='icon-sm'
        aria-label={t('navigator.pages')}
        aria-pressed={activePanel === 'pages'}
        onClick={() => togglePanel('pages')}
      >
        <Images />
      </Button>
      <Button
        variant={activePanel === 'inspector' ? 'secondary' : 'ghost'}
        size='icon-sm'
        aria-label={t('agent.properties')}
        aria-pressed={activePanel === 'inspector'}
        onClick={() => togglePanel('inspector')}
      >
        <SlidersHorizontal />
      </Button>
      <Button
        variant={activePanel === 'activity' ? 'secondary' : 'ghost'}
        size='icon-sm'
        className='relative'
        aria-label={t('activity.title')}
        aria-pressed={activePanel === 'activity'}
        disabled={activityCount === 0}
        onClick={() => togglePanel('activity')}
      >
        <Activity />
        {activityCount > 0 && (
          <span className='absolute -top-0.5 -right-0.5 grid size-3.5 place-items-center rounded-full bg-primary text-[8px] font-semibold text-primary-foreground tabular-nums'>
            {activityCount}
          </span>
        )}
      </Button>

      <DropdownMenu>
        <DropdownMenuTrigger
          render={<Button variant='ghost' size='icon-sm' aria-label={t('menu.more')} />}
        >
          <MoreVertical />
        </DropdownMenuTrigger>
        <DropdownMenuContent align='end' className='min-w-48'>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger disabled={!project || importing}>
              {importing ? <LoaderCircle className='animate-spin' /> : <FilePlus2 />}
              {t('menu.import')}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              <DropdownMenuItem disabled={importing} onClick={() => importPages('files')}>
                <FilePlus2 />
                {t('navigator.importFiles')}
              </DropdownMenuItem>
              <DropdownMenuItem disabled={importing} onClick={() => importPages('folder')}>
                <FolderOpen />
                {t('navigator.importFolder')}
              </DropdownMenuItem>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger disabled={!project || pages.length === 0 || exporting}>
              {exporting ? <LoaderCircle className='animate-spin' /> : <Download />}
              {t('menu.export')}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              {exportFormats.map((format) => (
                <DropdownMenuItem
                  key={format}
                  disabled={exporting}
                  onClick={() => exportProject(format)}
                >
                  {format.toUpperCase()}…
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuItem disabled={!page} onClick={share}>
            <Share2 />
            {t('menu.share')}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem disabled={!project?.can_undo} onClick={undo}>
            <Undo2 />
            {t('menu.undo')}
          </DropdownMenuItem>
          <DropdownMenuItem disabled={!project?.can_redo} onClick={redo}>
            <Redo2 />
            {t('menu.redo')}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={() => setSettingsOpen(true)}>
            <Settings />
            {t('menu.settings')}
          </DropdownMenuItem>
          <DropdownMenuItem disabled={!project} variant='destructive' onClick={closeProject}>
            <X />
            {t('menu.closeProject')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </header>
  )
}
