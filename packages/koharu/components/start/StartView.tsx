'use client'

import { Folder, FolderPlus, Plus, Settings, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { registerBackHandler } from '@/lib/back'
import { call } from '@/lib/backend'
import { pageKey, pagesKey, projectKey, refresh } from '@/lib/queries'
import { useKoharuStore } from '@/lib/store'
import { commands, type ProjectSummary } from '@koharu/bridge/protocol'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from '@koharu/ui/components/alert-dialog'
import { Button } from '@koharu/ui/components/button'
import { Input } from '@koharu/ui/components/input'
import { ScrollArea } from '@koharu/ui/components/scroll-area'
import { Tooltip, TooltipContent, TooltipTrigger } from '@koharu/ui/components/tooltip'

export function StartView() {
  const { t } = useTranslation()
  const setSettingsOpen = useKoharuStore((state) => state.setSettingsOpen)
  const platform = useKoharuStore((state) => state.platform)
  const [projects, setProjects] = useState<ProjectSummary[]>([])
  const [name, setName] = useState('')
  const [busy, setBusy] = useState<string | null>('list')
  const [projectToDelete, setProjectToDelete] = useState<string | null>(null)

  const reload = useCallback(async () => {
    setBusy('list')
    try {
      setProjects(await call(commands.listProjects))
    } finally {
      setBusy(null)
    }
  }, [])

  useEffect(() => {
    void reload().catch(() => undefined)
  }, [reload])

  // The confirmation dialog is a dismissible surface, so back closes it instead of
  // backgrounding the app.
  useEffect(() => {
    if (projectToDelete === null) return
    return registerBackHandler(() => {
      setProjectToDelete(null)
      return true
    })
  }, [projectToDelete])

  const createProject = async (event: FormEvent) => {
    event.preventDefault()
    const projectName = name.trim()
    if (!projectName || busy) return
    setBusy('create')
    try {
      await call(commands.createProject, projectName)
      await refresh(projectKey, pagesKey, pageKey)
      setName('')
    } finally {
      setBusy(null)
    }
  }

  const openProject = async (projectName: string) => {
    if (busy) return
    setBusy(projectName)
    try {
      await call(commands.openProject, projectName)
      await refresh(projectKey, pagesKey, pageKey)
    } finally {
      setBusy(null)
    }
  }

  const deleteProject = async () => {
    const projectName = projectToDelete
    if (busy || !projectName) return
    setBusy(projectName)
    try {
      await call(commands.deleteProject, projectName)
      await reload()
      setProjectToDelete(null)
    } finally {
      setBusy(null)
    }
  }

  return (
    <AlertDialog
      open={projectToDelete !== null}
      onOpenChange={(open) => {
        if (!open && busy === null) setProjectToDelete(null)
      }}
    >
      <main className='flex min-h-0 flex-1 flex-col overflow-hidden bg-[var(--surface-canvas)]'>
        <section
          className='mx-auto flex h-full min-h-0 w-full max-w-[960px] flex-col px-5 pt-5 pb-4 sm:px-10 sm:pt-10 sm:pb-6'
          aria-labelledby='start-title'
        >
          <header className='flex shrink-0 items-start justify-between gap-6'>
            <div>
              <h1 id='start-title' className='text-[24px] font-semibold tracking-[-0.03em]'>
                {t('start.title')}
              </h1>
              <p className='mt-1 text-[12px] leading-5 text-muted-foreground'>
                {t('start.description')}
              </p>
            </div>
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    type='button'
                    variant='ghost'
                    size='icon-sm'
                    className='size-8 shrink-0 text-muted-foreground hover:bg-foreground/[0.05] hover:text-foreground'
                    aria-label={t('menu.settings')}
                    onClick={() => setSettingsOpen(true)}
                  />
                }
              >
                <Settings className='size-4' />
              </TooltipTrigger>
              <TooltipContent side='bottom'>{t('menu.settings')}</TooltipContent>
            </Tooltip>
          </header>

          <div className='mt-5 grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] overflow-hidden rounded-2xl border border-border/80 bg-[var(--surface-panel)] md:mt-7 md:grid-cols-[288px_minmax(0,1fr)] md:grid-rows-1'>
            <aside className='flex flex-col border-b border-border/80 bg-[var(--surface-titlebar)]/55 p-3 md:border-r md:border-b-0 md:p-6'>
              <div className='flex items-center gap-2.5 md:flex-col md:items-start md:gap-0'>
                <span className='grid size-8 shrink-0 place-items-center rounded-lg bg-accent text-accent-foreground md:size-10 md:rounded-xl'>
                  <FolderPlus className='size-4 md:size-[18px]' />
                </span>
                <div className='min-w-0'>
                  <h2 className='text-[12px] font-semibold tracking-[-0.02em] md:mt-5 md:text-[15px]'>
                    {t('start.newProject')}
                  </h2>
                  <p className='mt-1 hidden max-w-[30ch] text-[11px] leading-[1.6] text-muted-foreground md:block'>
                    {t('start.newDescription')}
                  </p>
                </div>
              </div>

              <form
                className='mt-2.5 flex items-center gap-2 md:mt-6 md:grid md:gap-2.5'
                onSubmit={createProject}
              >
                <label
                  htmlFor='project-name'
                  className='hidden text-[10px] font-medium text-foreground md:block'
                >
                  {t('start.projectName')}
                </label>
                <Input
                  id='project-name'
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  placeholder={t('start.projectNamePlaceholder')}
                  aria-label={t('start.projectName')}
                  autoComplete='off'
                  disabled={busy !== null}
                  className='h-10 flex-1 bg-background text-[11px] md:h-9'
                />
                <Button
                  type='submit'
                  size='sm'
                  className='h-10 shrink-0 justify-center gap-1.5 px-4 text-[11px] md:h-9 md:w-full md:px-0'
                  disabled={!name.trim() || busy !== null}
                >
                  <Plus className='size-3.5' />
                  {t('start.create')}
                </Button>
              </form>

              <p className='mt-auto hidden pt-8 text-[10px] leading-4 text-muted-foreground md:block'>
                {t(platform === 'mobile' ? 'start.storageHintMobile' : 'start.storageHint')}
              </p>
            </aside>

            <section className='flex min-h-0 min-w-0 flex-col' aria-labelledby='project-list-title'>
              <header className='flex h-12 shrink-0 items-center border-b border-border/70 px-4 md:h-14 md:px-5'>
                <h2 id='project-list-title' className='text-[12px] font-semibold'>
                  {t('start.yourProjects')}
                </h2>
                <span className='ml-2 rounded-full bg-muted px-2 py-0.5 text-[9px] text-muted-foreground tabular-nums'>
                  {projects.length}
                </span>
              </header>

              <ScrollArea
                className='flex min-h-0 flex-1 flex-col'
                viewportClassName='min-h-0 flex-1 p-2'
                aria-busy={busy === 'list'}
              >
                {busy === 'list' && projects.length === 0 ? (
                  <p className='grid min-h-full place-items-center text-[11px] text-muted-foreground'>
                    {t('start.loadingProjects')}
                  </p>
                ) : projects.length === 0 ? (
                  <div
                    className='grid min-h-full place-items-center px-6 text-center'
                    role='status'
                  >
                    <div>
                      <span className='mx-auto grid size-11 place-items-center rounded-xl bg-muted text-muted-foreground'>
                        <Folder className='size-[18px]' />
                      </span>
                      <p className='mt-3 text-[12px] font-medium'>{t('start.emptyTitle')}</p>
                      <p className='mt-1 text-[10px] leading-4 text-muted-foreground'>
                        {t('start.emptyDescription')}
                      </p>
                    </div>
                  </div>
                ) : (
                  <ul className='grid gap-0.5' aria-label={t('start.projectList')}>
                    {projects.map((project) => (
                      <li
                        key={project.name}
                        className='group flex min-w-0 items-center rounded-lg hover:bg-foreground/[0.045]'
                      >
                        <button
                          type='button'
                          className='flex min-w-0 flex-1 items-center gap-3 rounded-lg px-3 py-2.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset'
                          onClick={() => void openProject(project.name).catch(() => undefined)}
                          disabled={busy !== null}
                        >
                          <span className='grid size-8 shrink-0 place-items-center rounded-lg bg-accent text-accent-foreground'>
                            <Folder className='size-4' />
                          </span>
                          <span className='min-w-0'>
                            <span className='block truncate text-[11px] font-medium'>
                              {project.name}
                            </span>
                            <span className='mt-0.5 block text-[9px] text-muted-foreground'>
                              {t('start.projectKind')}
                            </span>
                          </span>
                        </button>
                        <Button
                          type='button'
                          size='icon-sm'
                          variant='ghost'
                          className='mr-2 size-7 shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100 hover:text-foreground focus-visible:opacity-100 pointer-coarse:size-11 pointer-coarse:opacity-100'
                          aria-label={t('start.deleteLabel', { name: project.name })}
                          onClick={() => setProjectToDelete(project.name)}
                          disabled={busy !== null}
                        >
                          <Trash2 className='size-3.5' />
                        </Button>
                      </li>
                    ))}
                  </ul>
                )}
              </ScrollArea>
            </section>
          </div>
        </section>
      </main>

      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia className='bg-destructive/10 text-destructive'>
            <Trash2 className='size-5' />
          </AlertDialogMedia>
          <AlertDialogTitle>{t('start.deleteTitle')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('start.deleteDescription', { name: projectToDelete })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={busy !== null}>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction
            variant='destructive'
            disabled={busy !== null}
            aria-busy={busy !== null}
            onClick={() => void deleteProject().catch(() => undefined)}
          >
            {busy !== null ? t('start.deleting') : t('start.deleteAction')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
