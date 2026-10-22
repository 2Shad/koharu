'use client'

import { useQuery } from '@tanstack/react-query'
import { observeElementRect, useVirtualizer } from '@tanstack/react-virtual'
import {
  CheckSquare,
  FilePlus2,
  FolderOpen,
  GripVertical,
  LoaderCircle,
  MoreHorizontal,
  Search,
  Settings,
  Trash2,
} from 'lucide-react'
import {
  type PointerEvent as ReactPointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'

import { ResourceMonitor } from '@/components/editor/ResourceMonitor'
import { call } from '@/lib/backend'
import {
  pageKey,
  pagesKey,
  preparedPageKey,
  projectKey,
  queryClient,
  refresh,
  useImportPages,
  usePage,
  usePages,
} from '@/lib/queries'
import { useKoharuStore } from '@/lib/store'
import { prefetchCanvasPages, showCanvasPage } from '@koharu/bridge/canvas'
import {
  commands,
  type CanvasPagePreparation,
  type Page,
  type PageImportSource,
  type PageSummary,
  type ProjectInfo,
} from '@koharu/bridge/protocol'
import { Button } from '@koharu/ui/components/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@koharu/ui/components/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@koharu/ui/components/dropdown-menu'
import { Input } from '@koharu/ui/components/input'
import { ScrollArea } from '@koharu/ui/components/scroll-area'
import { Tooltip, TooltipContent, TooltipTrigger } from '@koharu/ui/components/tooltip'
import { cn } from '@koharu/ui/lib/utils'

const emptyPages: PageSummary[] = []

interface IntentPrefetchState {
  project: string
  revision: number
  pages: Set<string>
}

// Pointer-based reorder state. HTML5 drag events are unreliable on touch, so a
// finger drags from the handle and the row under the pointer becomes the target.
interface ReorderState {
  id: string
  from: number
  over: number
}

export function PageRail() {
  const { t } = useTranslation()
  const platform = useKoharuStore((state) => state.platform)
  const mobile = platform === 'mobile'
  const pages = usePages().data ?? emptyPages
  const active = usePage().data?.id ?? null
  const selected = useKoharuStore((state) => state.selectedPages)
  const selectPages = useKoharuStore((state) => state.selectPages)
  const selectLayers = useKoharuStore((state) => state.selectLayers)
  const setSettingsOpen = useKoharuStore((state) => state.setSettingsOpen)
  const { importPages, importing } = useImportPages()
  const anchor = useRef<number | null>(null)
  const selectionRequest = useRef(0)
  const intentPrefetch = useRef<IntentPrefetchState | null>(null)
  const reorderRef = useRef<ReorderState | null>(null)
  const [dragged, setDragged] = useState<string | null>(null)
  const [multiSelect, setMultiSelect] = useState(false)
  const [reorder, setReorder] = useState<ReorderState | null>(null)
  const [query, setQuery] = useState('')
  const [renaming, setRenaming] = useState<PageSummary | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const normalized = query.trim().toLocaleLowerCase()
  const visiblePages = useMemo(
    () =>
      pages
        .map((page, index) => ({ page, index }))
        .filter(({ page }) => !normalized || page.label.toLocaleLowerCase().includes(normalized)),
    [normalized, pages],
  )
  const pageList = useRef<HTMLDivElement>(null)
  const pageVirtualizer = useVirtualizer({
    count: visiblePages.length,
    getScrollElement: () => pageList.current,
    getItemKey: (index) => visiblePages[index]?.page.id ?? index,
    estimateSize: () => 72,
    gap: 2,
    overscan: 6,
    initialRect: { width: 240, height: 600 },
    observeElementRect: (instance, callback) =>
      observeElementRect(instance, (rect) =>
        callback({ width: rect.width || 240, height: rect.height || 600 }),
      ),
  })

  useEffect(
    () => () => {
      selectionRequest.current += 1
    },
    [],
  )

  const select = (index: number, additive: boolean, range: boolean) => {
    const page = pages[index]
    if (!page) return
    let next: string[]
    if (range && anchor.current !== null) {
      const start = Math.min(anchor.current, index)
      const end = Math.max(anchor.current, index)
      const rangeIds = pages.slice(start, end + 1).map((item) => item.id)
      next = additive ? [...new Set([...selected, ...rangeIds])] : rangeIds
    } else if (additive) {
      next = selected.includes(page.id)
        ? selected.filter((id) => id !== page.id)
        : [...selected, page.id]
      anchor.current = index
    } else {
      next = [page.id]
      anchor.current = index
    }
    const previousProject = queryClient.getQueryData<ProjectInfo | null>(projectKey)
    const previousPage = queryClient.getQueryData<Page | null>(pageKey)
    const prepared = queryClient.getQueryData<CanvasPagePreparation>(preparedPageKey(page.id))
    const activated = showCanvasPage(page.id, previousProject?.revision ?? null)
    const request = ++selectionRequest.current
    const synchronize = () => {
      if (selectionRequest.current !== request) return
      if (activated && previousProject && prepared?.revision === previousProject.revision) {
        queryClient.setQueryData(projectKey, { ...previousProject, active_page: page.id })
        queryClient.setQueryData(pageKey, prepared.page)
      }
      selectPages(next)
      selectLayers([])
      void call(commands.selectPage, page.id)
        .then((selection) => {
          if (selectionRequest.current !== request) return
          queryClient.setQueryData(projectKey, selection.project)
          queryClient.setQueryData(pageKey, selection.page)
        })
        .catch(() => {
          if (selectionRequest.current !== request) return
          if (queryClient.getQueryData<ProjectInfo | null>(projectKey)?.active_page === page.id) {
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

  const prefetchOnIntent = (page: string) => {
    const project = queryClient.getQueryData<ProjectInfo | null>(projectKey)
    if (!project || project.active_page === page) return
    let state = intentPrefetch.current
    if (!state || state.project !== project.name || state.revision !== project.revision) {
      state = { project: project.name, revision: project.revision, pages: new Set() }
      intentPrefetch.current = state
    }
    const prepared = queryClient.getQueryData<CanvasPagePreparation>(preparedPageKey(page))
    if (prepared?.revision === project.revision || state.pages.has(page)) return
    state.pages.add(page)
    void prefetchCanvasPages([page])
      .then((pages) => {
        const current = queryClient.getQueryData<ProjectInfo | null>(projectKey)
        const preparedPage = pages.find(
          (candidate) => candidate.page.id === page && candidate.revision === project.revision,
        )
        if (
          preparedPage &&
          current?.name === project.name &&
          current.revision === preparedPage.revision
        ) {
          queryClient.setQueryData(preparedPageKey(page), preparedPage)
          return
        }
        if (intentPrefetch.current === state) state.pages.delete(page)
      })
      .catch(() => {
        if (intentPrefetch.current === state) state.pages.delete(page)
      })
  }

  const updateReorder = (value: ReorderState | null) => {
    reorderRef.current = value
    setReorder(value)
  }

  const startReorder = (
    event: ReactPointerEvent<HTMLButtonElement>,
    id: string,
    index: number,
  ) => {
    if (!mobile) return
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    setDragged(id)
    updateReorder({ id, from: index, over: index })
  }

  const moveReorder = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const current = reorderRef.current
    if (!current) return
    const row = document
      .elementFromPoint(event.clientX, event.clientY)
      ?.closest<HTMLElement>('[data-page-index]')
    const over = row ? Number(row.dataset.pageIndex) : current.over
    if (!Number.isFinite(over) || over === current.over) return
    updateReorder({ ...current, over })
  }

  const endReorder = () => {
    const current = reorderRef.current
    updateReorder(null)
    setDragged(null)
    if (!current || current.over === current.from) return
    void call(commands.movePage, current.id, current.over)
      .then(() => refresh(projectKey, pagesKey))
      .catch(() => undefined)
  }

  const cancelReorder = () => {
    updateReorder(null)
    setDragged(null)
  }

  const deletePage = (page: string) =>
    void call(commands.deletePages, [page])
      .then(() => {
        selectPages(selected.filter((selectedPage) => selectedPage !== page))
        if (active === page) selectLayers([])
        return refresh(projectKey, pagesKey, pageKey)
      })
      .catch(() => undefined)

  const openRename = (page: PageSummary) => {
    setRenaming(page)
    setRenameValue(page.label)
  }

  const closeRename = () => {
    setRenaming(null)
    setRenameValue('')
  }

  const submitRename = () => {
    const page = renaming
    const label = renameValue.trim()
    if (!page || !label || label === page.label) {
      closeRename()
      return
    }
    void call(commands.renamePage, page.id, label)
      .then(() => refresh(projectKey, pagesKey, pageKey))
      .catch(() => undefined)
      .finally(closeRename)
  }

  return (
    <>
      <aside className='flex h-full min-h-0 flex-col bg-[var(--surface-sidebar)]'>
        <header className={cn('flex shrink-0 items-center px-2.5', mobile ? 'h-12' : 'h-10')}>
          <div className='flex min-w-0 items-center gap-2'>
            <h2 className='text-[11px] font-semibold'>{t('navigator.pages')}</h2>
            <span className='rounded-full bg-primary/[0.07] px-1.5 py-0.5 text-[9px] text-muted-foreground tabular-nums'>
              {pages.length}
            </span>
          </div>
          {/* One selected page behaves exactly like acting on the active
              page, so saying so would be noise. */}
          {selected.length > 1 && (
            <span
              role='status'
              aria-live='polite'
              className='ml-auto text-[9px] text-muted-foreground tabular-nums'
            >
              {t('navigator.selected', { count: selected.length })}
            </span>
          )}
          {mobile && (
            <Button
              type='button'
              variant={multiSelect ? 'secondary' : 'ghost'}
              size='icon'
              aria-label={t('navigator.multiSelect')}
              aria-pressed={multiSelect}
              className={cn(
                'size-11 shrink-0 text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground',
                selected.length <= 1 && 'ml-auto',
              )}
              onClick={() => setMultiSelect((value) => !value)}
            >
              <CheckSquare className='size-4' />
            </Button>
          )}
        </header>

        {pages.length > 0 && (
          <div className='border-b px-2 py-1.5'>
            <label
              className={cn(
                'flex items-center gap-1.5 rounded-md border border-input bg-background/70 px-1.5',
                mobile ? 'h-11' : 'h-6',
              )}
            >
              <Search className='size-3 text-muted-foreground' />
              <Input
                value={query}
                aria-label={t('navigator.filter')}
                placeholder={t('navigator.filter')}
                className={cn(
                  'min-w-0 border-0 bg-transparent p-0 text-[11px] shadow-none focus-visible:ring-0 md:text-[11px]',
                  mobile ? 'h-9' : 'h-5',
                )}
                onChange={(event) => setQuery(event.currentTarget.value)}
              />
            </label>
          </div>
        )}

        {pages.length === 0 ? (
          <div className='flex min-h-0 flex-1 flex-col items-center justify-center px-4 text-center'>
            <p className='text-[11px] font-medium'>{t('navigator.emptyTitle')}</p>
            <p className='mt-1 text-[10px] leading-4 text-muted-foreground'>
              {t('navigator.emptyDescription')}
            </p>
            <div className='mt-3'>
              <PageImportMenu importing={importing} onImport={importPages} />
            </div>
          </div>
        ) : visiblePages.length === 0 ? (
          <div className='grid flex-1 place-items-center px-4 text-center text-[10px] text-muted-foreground'>
            {t('navigator.noResults')}
          </div>
        ) : (
          <ScrollArea
            className='min-h-0 flex-1'
            viewportClassName='px-1.5 py-1.5'
            viewportRef={pageList}
          >
            <div className='relative w-full' style={{ height: pageVirtualizer.getTotalSize() }}>
              {pageVirtualizer.getVirtualItems().map((virtualRow) => {
                const item = visiblePages[virtualRow.index]
                if (!item) return null
                const { page, index } = item
                return (
                  <div
                    key={virtualRow.key}
                    data-index={virtualRow.index}
                    data-page-index={index}
                    className='absolute top-0 left-0 w-full'
                    style={{ transform: `translateY(${virtualRow.start}px)` }}
                  >
                    <PageItem
                      page={page}
                      mobile={mobile}
                      multiSelect={multiSelect}
                      active={active === page.id}
                      selected={selected.includes(page.id)}
                      dragged={dragged === page.id}
                      dropTarget={reorder !== null && reorder.over === index && reorder.id !== page.id}
                      onIntent={active === page.id ? undefined : () => prefetchOnIntent(page.id)}
                      onSelect={(additive, range) => select(index, additive, range)}
                      onDragStart={() => setDragged(page.id)}
                      onDragEnd={() => setDragged(null)}
                      onRename={() => openRename(page)}
                      onDelete={() => deletePage(page.id)}
                      onReorderStart={(event) => startReorder(event, page.id, index)}
                      onReorderMove={moveReorder}
                      onReorderEnd={endReorder}
                      onReorderCancel={cancelReorder}
                      onDrop={() => {
                        if (dragged && dragged !== page.id) {
                          void call(commands.movePage, dragged, index)
                            .then(() => refresh(projectKey, pagesKey))
                            .catch(() => undefined)
                        }
                        setDragged(null)
                      }}
                    />
                  </div>
                )
              })}
            </div>
          </ScrollArea>
        )}

        <div className='mt-auto flex shrink-0 items-center gap-2 px-2 py-2'>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  type='button'
                  variant='ghost'
                  size='icon'
                  className={cn(
                    'text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground',
                    mobile ? 'size-11' : 'size-8',
                  )}
                  aria-label={t('menu.settings')}
                  onClick={() => setSettingsOpen(true)}
                />
              }
            >
              <Settings className='size-4' />
            </TooltipTrigger>
            <TooltipContent side='right'>{t('menu.settings')}</TooltipContent>
          </Tooltip>
          <ResourceMonitor />
        </div>
      </aside>

      <Dialog
        open={renaming !== null}
        onOpenChange={(open) => {
          if (!open) closeRename()
        }}
      >
        <DialogContent className='sm:max-w-md'>
          <form
            onSubmit={(event) => {
              event.preventDefault()
              submitRename()
            }}
          >
            <DialogHeader>
              <DialogTitle>{t('navigator.renameTitle')}</DialogTitle>
              <DialogDescription>{t('navigator.renameDescription')}</DialogDescription>
            </DialogHeader>
            <Input
              autoFocus
              value={renameValue}
              aria-label={t('navigator.pageName')}
              className='mt-4'
              onChange={(event) => setRenameValue(event.currentTarget.value)}
            />
            <DialogFooter className='mt-4'>
              <Button type='button' variant='ghost' onClick={closeRename}>
                {t('common.cancel')}
              </Button>
              <Button type='submit' disabled={!renameValue.trim()}>
                {t('navigator.rename')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  )
}

function PageImportMenu({
  importing,
  onImport,
}: {
  importing: boolean
  onImport: (source: PageImportSource) => void
}) {
  const { t } = useTranslation()
  const mobile = useKoharuStore((state) => state.platform) === 'mobile'
  const icon = importing ? (
    <LoaderCircle className='size-3.5 animate-spin' />
  ) : (
    <FilePlus2 className='size-3.5' />
  )

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            size='sm'
            variant='outline'
            disabled={importing}
            aria-busy={importing}
            className={cn('rounded-lg text-[10px]', mobile && 'h-11 px-4')}
          />
        }
      >
        {icon}
        {importing ? t('navigator.importingAction') : t('navigator.importAction')}
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align='start'
        className='w-auto min-w-20 border border-border/50 p-0.5 shadow-sm ring-0'
      >
        <DropdownMenuItem
          disabled={importing}
          className={cn(
            'gap-1 px-1.5 py-0.5 text-[11px] [&_svg:not([class*="size-"])]:size-3.5',
            mobile ? 'min-h-11' : 'min-h-7',
          )}
          onClick={() => onImport('files')}
        >
          <FilePlus2 />
          {t('navigator.importFiles')}
        </DropdownMenuItem>
        <DropdownMenuItem
          disabled={importing}
          className={cn(
            'gap-1 px-1.5 py-0.5 text-[11px] [&_svg:not([class*="size-"])]:size-3.5',
            mobile ? 'min-h-11' : 'min-h-7',
          )}
          onClick={() => onImport('folder')}
        >
          <FolderOpen />
          {t('navigator.importFolder')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function PageItem({
  page,
  mobile,
  multiSelect,
  active,
  selected,
  dragged,
  dropTarget,
  onIntent,
  onSelect,
  onDragStart,
  onDragEnd,
  onRename,
  onDelete,
  onDrop,
  onReorderStart,
  onReorderMove,
  onReorderEnd,
  onReorderCancel,
}: {
  page: PageSummary
  mobile: boolean
  multiSelect: boolean
  active: boolean
  selected: boolean
  dragged: boolean
  dropTarget: boolean
  onIntent?: () => void
  onSelect: (additive: boolean, range: boolean) => void
  onDragStart: () => void
  onDragEnd: () => void
  onRename: () => void
  onDelete: () => void
  onDrop: () => void
  onReorderStart: (event: ReactPointerEvent<HTMLButtonElement>) => void
  onReorderMove: (event: ReactPointerEvent<HTMLButtonElement>) => void
  onReorderEnd: () => void
  onReorderCancel: () => void
}) {
  const { t } = useTranslation()

  return (
    <article
      draggable={!mobile}
      data-active={active}
      data-selected={selected}
      data-drop-target={dropTarget || undefined}
      className={cn(
        'group grid cursor-default grid-cols-[48px_minmax(0,1fr)] gap-2.5 rounded-xl p-1.5 transition-colors select-none',
        active
          ? 'bg-primary/[0.09] hover:bg-primary/[0.09]'
          : selected
            ? 'bg-foreground/[0.06] hover:bg-foreground/[0.08]'
            : 'hover:bg-foreground/[0.045]',
        dragged && 'opacity-50',
        dropTarget && 'ring-2 ring-primary/60',
      )}
      onPointerEnter={onIntent}
      onFocus={onIntent}
      onClick={(event) => {
        if ((event.target as HTMLElement).closest('button,[role="menuitem"]')) return
        onSelect(multiSelect || event.ctrlKey || event.metaKey, event.shiftKey)
      }}
      onDoubleClick={mobile ? undefined : onRename}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault()
        onDrop()
      }}
    >
      <div className='grid h-16 w-12 place-items-center overflow-hidden rounded-lg bg-[var(--surface-well)]'>
        {page.source_asset ? (
          <PageThumbnail page={page.id} asset={page.source_asset} label={page.label} />
        ) : (
          <span className='text-[9px] text-muted-foreground'>{t('navigator.noImage')}</span>
        )}
      </div>
      <div className='min-w-0 py-0.5'>
        <div className='flex items-start gap-1'>
          <span className='min-w-0 flex-1 truncate text-[10px] font-medium'>{page.label}</span>
          {mobile && (
            <button
              type='button'
              aria-label={t('navigator.reorder', { page: page.label })}
              className='grid size-11 shrink-0 touch-none place-items-center rounded-md text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground'
              onPointerDown={onReorderStart}
              onPointerMove={onReorderMove}
              onPointerUp={onReorderEnd}
              onPointerCancel={onReorderCancel}
            >
              <GripVertical className='size-4' />
            </button>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  variant='ghost'
                  size='icon-xs'
                  aria-label={t('navigator.actionsFor', { page: page.label })}
                  className={cn(
                    'shrink-0 shadow-none',
                    mobile
                      ? 'size-11'
                      : '-mt-1 opacity-0 group-hover:opacity-100 aria-expanded:opacity-100',
                  )}
                />
              }
            >
              <MoreHorizontal />
            </DropdownMenuTrigger>
            <DropdownMenuContent align='end'>
              <DropdownMenuItem className={cn(mobile && 'min-h-11')} onClick={onRename}>
                {t('navigator.rename')}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                variant='destructive'
                className={cn(mobile && 'min-h-11')}
                onClick={onDelete}
              >
                <Trash2 /> {t('navigator.delete')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <p className='mt-1 text-[9px] leading-3.5 text-muted-foreground'>
          {t('navigator.layerCount', { count: page.layer_count })}
        </p>
        <p className='mt-1 text-[9px] text-muted-foreground tabular-nums'>
          {page.size.width} × {page.size.height}
        </p>
      </div>
    </article>
  )
}

function PageThumbnail({ page, asset, label }: { page: string; asset: string; label: string }) {
  const { t } = useTranslation()
  const [source, setSource] = useState<string | null>(null)
  const [settled, setSettled] = useState(false)

  useEffect(() => {
    setSettled(false)
    const timeout = window.setTimeout(() => setSettled(true), 100)
    return () => window.clearTimeout(timeout)
  }, [asset])

  const thumbnail = useQuery({
    queryKey: ['thumbnail', asset],
    queryFn: async () => new Uint8Array(await call(commands.getThumbnail, page)),
    enabled: settled,
    staleTime: Number.POSITIVE_INFINITY,
    notifyOnChangeProps: ['data'],
  }).data

  useEffect(() => {
    if (!thumbnail) return
    const url = URL.createObjectURL(new Blob([thumbnail.buffer], { type: 'image/webp' }))
    setSource(url)
    return () => {
      URL.revokeObjectURL(url)
    }
  }, [thumbnail])

  return (
    <div className='grid size-full place-items-center'>
      {source ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img src={source} alt={label} draggable={false} className='size-full object-contain' />
      ) : (
        <span className='text-[9px] text-muted-foreground'>{t('common.loading')}</span>
      )}
    </div>
  )
}
