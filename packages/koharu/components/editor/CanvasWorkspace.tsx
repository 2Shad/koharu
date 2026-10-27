'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useColorSampling } from '@/components/controls/ColorSampling'
import { CanvasCommandBar } from '@/components/editor/CanvasCommandBar'
import { CanvasOverlay } from '@/components/editor/CanvasOverlay'
import { StatusBar } from '@/components/editor/StatusBar'
import { ToolBar } from '@/components/editor/ToolBar'
import { useCanvas } from '@/components/editor/useCanvas'
import { call } from '@/lib/backend'
import { expandLayerSelection } from '@/lib/document'
import {
  controlFrame,
  draftFrame,
  hitTestLayers,
  pagePoint,
  physicalPoint,
  selectableLayer,
  translateFrames,
} from '@/lib/geometry'
import { activatePage } from '@/lib/pageSelection'
import {
  pageKey,
  pagesKey,
  preparedPageKey,
  projectKey,
  queryClient,
  refresh,
  usePage,
  usePages,
} from '@/lib/queries'
import {
  isBrushTool,
  maxBrushDiameter,
  MIN_BRUSH_DIAMETER,
  receiveError,
  useKoharuStore,
  type CanvasTool,
} from '@/lib/store'
import { prefetchCanvasPages, workspaceColor, type CanvasColor } from '@koharu/bridge/canvas'
import { commands, type Frame, type Point, type TransformFrame } from '@koharu/bridge/protocol'
import { Button } from '@koharu/ui/components/button'

const BRUSH_DIAMETER_STEP = 4
// Touch-only tolerances. Pointer type (not platform) selects the touch path, so a mouse or
// trackpad on either platform keeps the exact desktop behavior.
const TOUCH_SLOP = 8
const LONG_PRESS_MS = 500
const PAGE_SWIPE_DISTANCE = 64

const canvasCursors = {
  select: undefined,
  text: 'text',
  draw: 'none',
  eraser: 'none',
  color_picker: 'crosshair',
  remove: 'none',
  pan: 'grab',
} as const satisfies Record<CanvasTool, string | undefined>

type Gesture =
  | { kind: 'pan'; pointer: number; start: Point; translation: [number, number] }
  | { kind: 'move'; pointer: number; start: Point; originals: TransformFrame[] }
  | { kind: 'text'; pointer: number; start: Point; frame: Frame }
  | { kind: 'page'; pointer: number; startClientX: number; startClientY: number }
  | {
      kind: 'pinch'
      pointers: [number, number]
      startDistance: number
      startZoom: number
      startMidpoint: Point
      startTranslation: [number, number]
    }
  | StrokeGesture

interface StrokeGesture {
  kind: 'paint' | 'erase' | 'inpaint'
  pointer: number
  revision: number
  layer: string | null
  points: Point[]
  diameter: number
  color?: CanvasColor
}

interface CanvasView {
  zoom: number
  translation: [number, number]
}

interface StrokeUpdate {
  kind: 'paint' | 'erase' | 'inpaint'
  points: Point[]
}

// A single-finger touch before it has moved past the slop threshold. Nothing is painted or
// dragged until this is promoted, so a tap cannot leave a mark.
interface PendingTouch {
  pointer: number
  start: Point
  physical: Point
  clientX: number
  clientY: number
}

export function CanvasWorkspace() {
  const { t } = useTranslation()
  const surface = useRef<HTMLDivElement>(null)
  const [canvasElement, setCanvasElement] = useState<HTMLCanvasElement | null>(null)
  const gesture = useRef<Gesture | null>(null)
  const pointers = useRef(new Map<number, { clientX: number; clientY: number }>())
  const pendingTouch = useRef<PendingTouch | null>(null)
  const longPress = useRef<number | null>(null)
  const pageContent = useRef<HTMLDivElement>(null)
  const pageAnimation = useRef<Animation | null>(null)
  const pageFollowOffset = useRef(0)
  const pageEntryOffset = useRef<number | null>(null)
  const previousPageIndex = useRef<number | null>(null)
  const previousPageId = useRef<string | null>(null)
  const spaceHeld = useRef(false)
  const transformActive = useRef(false)
  const transformRevision = useRef<number | null>(null)
  const transformFinal = useRef<TransformFrame[]>([])
  const commitPending = useRef(false)
  const commandQueue = useRef<Promise<void>>(Promise.resolve())
  const [previews, setPreviews] = useState<Record<string, Frame>>({})
  const [draft, setDraft] = useState<Frame | null>(null)
  const [hovered, setHovered] = useState<string | null>(null)
  const [cursor, setCursor] = useState<Point | null>(null)
  const colorSampling = useColorSampling()

  const page = usePage().data
  const pages = usePages().data
  const canvasPage = useKoharuStore((state) => state.canvasPage)
  const canvasRevision = useKoharuStore((state) => state.canvasRevision)
  const canvasGeneration = useKoharuStore((state) => state.canvasGeneration)
  const canvasSize = useKoharuStore((state) => state.canvasSize)
  const fitCanvasRequest = useKoharuStore((state) => state.fitCanvasRequest)
  const layerFrames = useKoharuStore((state) => state.layerFrames)
  const tool = useKoharuStore((state) => state.tool)
  const brush = useKoharuStore((state) => state.brush)
  const selected = useKoharuStore((state) => state.selectedLayers)
  const selectLayers = useKoharuStore((state) => state.selectLayers)
  const setTool = useKoharuStore((state) => state.setTool)
  const setBrush = useKoharuStore((state) => state.setBrush)
  const requestCanvasFit = useKoharuStore((state) => state.requestCanvasFit)
  const canvasState = useCanvas(canvasElement, canvasRevision, canvasGeneration)
  const canvas = canvasState.canvas
  const pageId = page?.id
  const pageWidth = canvasSize[0] || page?.size.width
  const pageHeight = canvasSize[1] || page?.size.height
  const activeRaster =
    selected.length === 1
      ? page?.layers.find((layer) => layer.id === selected[0] && layer.type === 'raster')
      : undefined

  // A swipe moves the page directly instead of waiting for the selected page to load. Direct
  // style writes keep pointer movement off the React render path; the page-change entrance
  // animation takes over only after the finger is released.
  const pageContentWidth = useCallback((content: HTMLElement): number => {
    const parent = content.parentElement
    return Math.max(
      1,
      content.clientWidth ||
        parent?.clientWidth ||
        parent?.getBoundingClientRect().width ||
        window.innerWidth,
    )
  }, [])

  const clearPageFollow = useCallback(() => {
    pageFollowOffset.current = 0
    const content = pageContent.current
    if (!content) return
    content.style.transform = ''
    content.style.opacity = ''
    content.style.willChange = ''
  }, [])

  const followPage = useCallback(
    (offset: number) => {
      const content = pageContent.current
      if (!content) return
      pageAnimation.current?.cancel()
      pageAnimation.current = null
      const width = pageContentWidth(content)
      const limit = width * 0.5
      const clamped = Math.max(-limit, Math.min(limit, offset))
      pageFollowOffset.current = clamped
      content.style.transform = `translate3d(${clamped}px, 0, 0)`
      content.style.opacity = `${1 - (Math.abs(clamped) / width) * 0.2}`
      content.style.willChange = 'transform, opacity'
    },
    [pageContentWidth],
  )

  const settlePageFollow = useCallback(() => {
    const content = pageContent.current
    const offset = pageFollowOffset.current
    if (!content || offset === 0) {
      clearPageFollow()
      return
    }
    pageAnimation.current?.cancel()
    pageAnimation.current = null
    if (
      typeof content.animate !== 'function' ||
      window.matchMedia('(prefers-reduced-motion: reduce)').matches
    ) {
      clearPageFollow()
      return
    }
    const animation = content.animate(
      [
        {
          opacity: Number(content.style.opacity || 1),
          transform: `translate3d(${offset}px, 0, 0)`,
        },
        { opacity: 1, transform: 'translate3d(0, 0, 0)' },
      ],
      { duration: 180, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' },
    )
    pageAnimation.current = animation
    pageFollowOffset.current = 0
    animation.onfinish = () => {
      if (pageAnimation.current !== animation) return
      pageAnimation.current = null
      clearPageFollow()
    }
  }, [clearPageFollow])

  const navigatePage = useCallback(
    (direction: -1 | 1): boolean => {
      if (!pageId || !pages?.length) return false
      const index = pages.findIndex((candidate) => candidate.id === pageId)
      if (index < 0) return false
      const target = pages[index + direction]
      if (!target) return false
      activatePage(target.id, [target.id])
      return true
    },
    [pageId, pages],
  )

  const animatePageChange = useCallback(
    (direction: -1 | 1) => {
      const content = pageContent.current
      const entryOffset = pageEntryOffset.current
      pageEntryOffset.current = null
      pageAnimation.current?.cancel()
      pageAnimation.current = null
      if (!content) return
      const width = pageContentWidth(content)
      clearPageFollow()
      if (
        typeof content.animate !== 'function' ||
        window.matchMedia('(prefers-reduced-motion: reduce)').matches
      )
        return
      const from =
        entryOffset === null
          ? direction * 26
          : Math.max(-width * 0.5, Math.min(width * 0.5, -entryOffset))
      const animation = content.animate(
        [
          { opacity: entryOffset === null ? 0.4 : 0.6, transform: `translate3d(${from}px, 0, 0)` },
          { opacity: 1, transform: 'translate3d(0, 0, 0)' },
        ],
        { duration: 240, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' },
      )
      pageAnimation.current = animation
      animation.onfinish = () => {
        if (pageAnimation.current === animation) pageAnimation.current = null
      }
    },
    [clearPageFollow, pageContentWidth],
  )

  const enqueue = useCallback(<Result,>(operation: () => Promise<Result>): Promise<Result> => {
    const pending = commandQueue.current.then(operation)
    commandQueue.current = pending.then(
      () => undefined,
      () => undefined,
    )
    return pending
  }, [])

  const transformUpdates = useFrameCommand((elements: TransformFrame[]) =>
    canvas?.updateTransform(elements),
  )
  const strokeUpdates = useFrameCommand(
    ({ points }: StrokeUpdate) => canvas?.extendStroke(points),
    mergeStrokeUpdates,
  )

  const beginTransform = useCallback(
    (elements: TransformFrame[]) => {
      if (
        !canvas ||
        canvasRevision === null ||
        !elements.length ||
        transformActive.current ||
        commitPending.current
      )
        return
      transformUpdates.clear()
      transformActive.current = true
      transformRevision.current = canvasRevision
      transformFinal.current = elements
      setPreviews(Object.fromEntries(elements.map(({ element, frame }) => [element, frame])))
      try {
        canvas.beginTransform(elements)
      } catch (error) {
        transformActive.current = false
        receiveError(errorMessage(error))
      }
    },
    [canvas, canvasRevision, transformUpdates],
  )

  const updateTransform = useCallback(
    (elements: TransformFrame[]) => {
      if (!transformActive.current) return
      transformFinal.current = elements
      setPreviews(Object.fromEntries(elements.map(({ element, frame }) => [element, frame])))
      transformUpdates.schedule(elements)
    },
    [transformUpdates],
  )

  const finishTransform = useCallback(() => {
    if (!transformActive.current) return
    transformUpdates.commit()
    transformActive.current = false
    const revision = transformRevision.current
    const elements = transformFinal.current
    transformRevision.current = null
    try {
      canvas?.finishTransform()
    } catch (error) {
      receiveError(errorMessage(error))
      setPreviews({})
      return
    }
    if (revision === null) {
      canvas?.cancelTransform()
      setPreviews({})
      return
    }
    commitPending.current = true
    void enqueue(() => call(commands.commitTransform, revision, elements))
      .then((revision) => (revision === null ? undefined : refresh(projectKey, pagesKey, pageKey)))
      .catch(() => canvas?.cancelTransform())
      .finally(() => {
        commitPending.current = false
        setPreviews({})
      })
  }, [canvas, enqueue, transformUpdates])

  const cancelGesture = useCallback(() => {
    if (longPress.current !== null) {
      window.clearTimeout(longPress.current)
      longPress.current = null
    }
    pendingTouch.current = null
    pointers.current.clear()
    const current = gesture.current
    gesture.current = null
    if (current?.kind === 'page') clearPageFollow()
    if (current?.kind === 'paint' || current?.kind === 'erase' || current?.kind === 'inpaint') {
      strokeUpdates.clear()
      canvas?.cancelStroke()
    }
    if (transformActive.current) {
      transformUpdates.clear()
      transformActive.current = false
      transformRevision.current = null
      canvas?.cancelTransform()
    }
    setDraft(null)
    setPreviews({})
  }, [canvas, clearPageFollow, strokeUpdates, transformUpdates])

  const fitCanvas = useCallback(() => {
    const element = surface.current
    if (!element || !pageId || pageWidth === undefined || pageHeight === undefined) return
    const bounds = element.getBoundingClientRect()
    const dpr = window.devicePixelRatio
    const next = containCamera(bounds.width * dpr, bounds.height * dpr, pageWidth, pageHeight)
    useKoharuStore.setState({ camera: { ...next, fitted: true } })
  }, [pageHeight, pageId, pageWidth])

  const report = useCallback(() => {
    const element = surface.current
    if (!element || !canvas) return
    const bounds = element.getBoundingClientRect()
    canvas.resize(bounds.width, bounds.height, window.devicePixelRatio, workspaceColor())
    if (useKoharuStore.getState().camera.fitted) fitCanvas()
  }, [canvas, fitCanvas])

  const setZoom = useCallback((zoom: number) => {
    const element = surface.current
    if (!element) return
    const bounds = element.getBoundingClientRect()
    const dpr = window.devicePixelRatio
    const current = useKoharuStore.getState().camera
    const center = { x: bounds.width * dpr * 0.5, y: bounds.height * dpr * 0.5 }
    const pageX = (center.x - current.translation[0]) / current.zoom
    const pageY = (center.y - current.translation[1]) / current.zoom
    useKoharuStore.setState({
      camera: {
        zoom,
        translation: [center.x - pageX * zoom, center.y - pageY * zoom],
        fitted: false,
      },
    })
  }, [])

  useEffect(() => {
    const element = surface.current
    if (!element) return
    report()
    const resize = new ResizeObserver(report)
    const theme = new MutationObserver(report)
    resize.observe(element)
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
    window.addEventListener('resize', report)
    window.visualViewport?.addEventListener('resize', report)
    return () => {
      resize.disconnect()
      theme.disconnect()
      window.removeEventListener('resize', report)
      window.visualViewport?.removeEventListener('resize', report)
    }
  }, [report])

  // The camera reaches the canvas from the store, not from a render: a passive effect lands a
  // frame late under continuous input and halves the presented rate.
  useEffect(() => {
    if (!canvas) return
    const apply = ({ zoom, translation }: CanvasView) => {
      try {
        canvas.setView(zoom, translation)
      } catch (error) {
        receiveError(errorMessage(error))
      }
    }
    apply(useKoharuStore.getState().camera)
    return useKoharuStore.subscribe((state, previous) => {
      if (state.camera !== previous.camera) apply(state.camera)
    })
  }, [canvas])

  useEffect(() => {
    if (
      !pageId ||
      canvasPage !== pageId ||
      canvasState.generation !== canvasGeneration ||
      canvasState.status !== 'ready' ||
      !pages?.length
    )
      return
    const index = pages.findIndex((candidate) => candidate.id === pageId)
    if (index < 0) return
    const previous = previousPageIndex.current
    const previousId = previousPageId.current
    previousPageIndex.current = index
    previousPageId.current = pageId
    if (previousId !== null && previousId !== pageId) {
      animatePageChange(previous !== null && index < previous ? -1 : 1)
    }
    const direction = previous !== null && index < previous ? -1 : 1
    const adjacent = pages[index + direction]?.id ?? pages[index - direction]?.id
    if (adjacent) {
      void prefetchCanvasPages([adjacent])
        .then((prepared) => {
          for (const page of prepared) {
            queryClient.setQueryData(preparedPageKey(page.page.id), page)
          }
        })
        .catch(() => undefined)
    }
  }, [
    animatePageChange,
    canvasGeneration,
    canvasPage,
    canvasState.generation,
    canvasState.status,
    pageId,
    pages,
  ])

  useEffect(() => {
    fitCanvas()
  }, [fitCanvas, fitCanvasRequest])

  useEffect(() => () => pageAnimation.current?.cancel(), [])

  useEffect(() => cancelGesture, [cancelGesture, canvasGeneration, canvasRevision, page?.id, tool])

  useEffect(() => {
    const editable = (target: EventTarget | null) =>
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      (target instanceof HTMLElement && target.isContentEditable)

    const down = (event: KeyboardEvent) => {
      if (event.key === 'Alt') {
        event.preventDefault()
        return
      }
      if (editable(event.target)) return
      const state = useKoharuStore.getState()
      if (event.code === 'Space') {
        spaceHeld.current = true
        event.preventDefault()
        return
      }
      const command = event.ctrlKey || event.metaKey
      if (command && event.key.toLowerCase() === 'z') {
        event.preventDefault()
        void call(event.shiftKey ? commands.redo : commands.undo)
          .then(() => refresh(projectKey, pagesKey, pageKey))
          .catch(() => undefined)
        return
      }
      if (command && event.key.toLowerCase() === 'a' && page) {
        event.preventDefault()
        selectLayers(page.layers.filter(selectableLayer).map((layer) => layer.id))
        return
      }
      if (
        (event.key === 'Delete' || event.key === 'Backspace') &&
        state.selectedLayers.length > 0
      ) {
        event.preventDefault()
        void call(commands.deleteLayers, state.selectedLayers)
          .then(() => refresh(projectKey, pagesKey, pageKey))
          .catch(() => undefined)
        return
      }
      if (event.key.toLowerCase() === state.shortcuts.fit) {
        requestCanvasFit()
        return
      }
      if (event.key === 'Escape') {
        cancelGesture()
        colorSampling?.cancel()
        selectLayers([])
        return
      }
      const next = (
        ['select', 'text', 'draw', 'eraser', 'color_picker', 'remove', 'pan'] as const
      ).find((action) => state.shortcuts[action] === event.key.toLowerCase())
      if (next) setTool(next)
    }

    const up = (event: KeyboardEvent) => {
      if (event.key === 'Alt') event.preventDefault()
      if (event.code === 'Space') spaceHeld.current = false
    }
    const blur = () => {
      spaceHeld.current = false
      cancelGesture()
    }

    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    window.addEventListener('blur', blur)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
      window.removeEventListener('blur', blur)
    }
  }, [cancelGesture, colorSampling, page, requestCanvasFit, selectLayers, setTool])

  const clientPagePoint = (clientX: number, clientY: number) =>
    pagePoint(
      clientX,
      clientY,
      surface.current!.getBoundingClientRect(),
      useKoharuStore.getState().camera,
    )

  const clientPhysicalPoint = (clientX: number, clientY: number) =>
    physicalPoint(clientX, clientY, surface.current!.getBoundingClientRect())

  const framesFor = (layers: string[]): TransformFrame[] =>
    expandLayerSelection(page?.layers ?? [], layers).flatMap((id) => {
      const layer = page?.layers.find((candidate) => candidate.id === id)
      const frame = layer && selectableLayer(layer) ? controlFrame(layer, layerFrames) : null
      return frame ? [{ element: id, frame }] : []
    })

  const moveGesture = (
    pointer: number,
    samples: ReadonlyArray<{ clientX: number; clientY: number }>,
  ) => {
    const current = gesture.current
    const sample = samples.at(-1)
    if (!page || !sample) return
    if (current?.kind === 'pinch') return
    const physical = clientPhysicalPoint(sample.clientX, sample.clientY)
    setCursor(physical)
    if (!current || current.pointer !== pointer) {
      if (tool === 'select') {
        setHovered(
          hitTestLayers(page.layers, clientPagePoint(sample.clientX, sample.clientY), layerFrames)
            ?.id ?? null,
        )
      }
      return
    }

    if (current.kind === 'pan') {
      const { zoom } = useKoharuStore.getState().camera
      const bounds = surface.current!.getBoundingClientRect()
      const dpr = window.devicePixelRatio
      let translation: [number, number] = [
        current.translation[0] + physical.x - current.start.x,
        current.translation[1] + physical.y - current.start.y,
      ]
      translation = clampCameraTranslation(
        translation,
        zoom,
        pageWidth ?? page.size.width,
        pageHeight ?? page.size.height,
        bounds.width * dpr,
        bounds.height * dpr,
        dpr,
      )
      useKoharuStore.setState({ camera: { zoom, translation, fitted: false } })
      return
    }

    if (current.kind === 'page') {
      followPage(sample.clientX - current.startClientX)
      return
    }

    const points = samples.map((value) => clientPagePoint(value.clientX, value.clientY))
    const point = points.at(-1)!
    if (current.kind === 'move') {
      updateTransform(
        translateFrames(current.originals, {
          x: point.x - current.start.x,
          y: point.y - current.start.y,
        }),
      )
    } else if (current.kind === 'text') {
      current.frame = draftFrame(current.start, point)
      setDraft(current.frame)
    } else if (current.kind === 'paint' || current.kind === 'erase' || current.kind === 'inpaint') {
      current.points.push(...points)
      strokeUpdates.schedule({ kind: current.kind, points })
    }
  }

  const finishGesture = () => {
    const current = gesture.current
    gesture.current = null
    if (!current || !page) return
    if (current.kind === 'move') {
      finishTransform()
    } else if (current.kind === 'text') {
      const { zoom } = useKoharuStore.getState().camera
      const pointText = current.frame.width < 4 / zoom && current.frame.height < 4 / zoom
      setDraft(null)
      void (
        pointText
          ? call(commands.addPointText, current.start)
          : call(commands.addTextBox, current.frame)
      )
        .then((result) => {
          selectLayers([result.layer])
          return refresh(projectKey, pagesKey, pageKey)
        })
        .catch(() => undefined)
    } else if (current.kind === 'paint' || current.kind === 'erase' || current.kind === 'inpaint') {
      strokeUpdates.commit()
      try {
        canvas?.finishStroke()
      } catch (error) {
        receiveError(errorMessage(error))
        return
      }
      const operation =
        current.kind === 'paint'
          ? enqueue(() =>
              call(commands.commitPaint, current.revision, current.layer, current.points, {
                diameter: current.diameter,
                color: current.color!,
              }),
            ).then((result) => {
              selectLayers([result.layer])
              return refresh(projectKey, pagesKey, pageKey)
            })
          : current.kind === 'erase'
            ? enqueue(() =>
                call(
                  commands.commitErase,
                  current.revision,
                  current.layer!,
                  current.points,
                  current.diameter,
                ),
              ).then((result) => {
                selectLayers([result.layer])
                return refresh(projectKey, pagesKey, pageKey)
              })
            : enqueue(() =>
                call(commands.commitInpaint, current.revision, current.points, current.diameter),
              )
      commitPending.current = true
      void operation
        .catch(() => canvas?.cancelStroke())
        .finally(() => (commitPending.current = false))
    }
  }

  const clearLongPress = () => {
    if (longPress.current !== null) {
      window.clearTimeout(longPress.current)
      longPress.current = null
    }
  }

  // Shared by mouse input and by a touch once it clears the slop threshold. `point` is always
  // the gesture origin, so a promoted touch starts exactly where the finger landed.
  const beginGesture = (
    pointer: number,
    point: Point,
    physical: Point,
    additive: boolean,
    middle: boolean,
  ) => {
    if (!page || !canvas || canvasRevision === null) return
    if (middle || tool === 'pan' || spaceHeld.current) {
      gesture.current = {
        kind: 'pan',
        pointer,
        start: physical,
        translation: useKoharuStore.getState().camera.translation,
      }
      return
    }
    if (tool === 'select') {
      const target = hitTestLayers(page.layers, point, layerFrames)
      if (!target) {
        if (!additive) selectLayers([])
        return
      }
      const next = additive
        ? selected.includes(target.id)
          ? selected.filter((id) => id !== target.id)
          : [...selected, target.id]
        : selected.includes(target.id)
          ? selected
          : [target.id]
      selectLayers(next)
      if (!next.includes(target.id)) return
      const originals = framesFor(next)
      if (!originals.length) return
      gesture.current = { kind: 'move', pointer, start: point, originals }
      beginTransform(originals)
      return
    }
    if (tool === 'text') {
      const frame = draftFrame(point, point)
      gesture.current = { kind: 'text', pointer, start: point, frame }
      setDraft(frame)
      return
    }
    if (tool === 'draw') {
      strokeUpdates.clear()
      const color = hexToRgba(brush.color)
      gesture.current = {
        kind: 'paint',
        pointer,
        revision: canvasRevision,
        layer: activeRaster?.id ?? null,
        points: [point],
        diameter: brush.diameter,
        color,
      }
      try {
        canvas.beginStroke({
          kind: 'paint',
          layer: activeRaster?.id ?? null,
          point,
          diameter: brush.diameter,
          color,
        })
      } catch (error) {
        gesture.current = null
        receiveError(errorMessage(error))
      }
      return
    }
    if (tool === 'eraser') {
      if (!activeRaster) {
        receiveError('Select a paint or cleanup layer before using the Eraser.')
        return
      }
      strokeUpdates.clear()
      gesture.current = {
        kind: 'erase',
        pointer,
        revision: canvasRevision,
        layer: activeRaster.id,
        points: [point],
        diameter: brush.diameter,
      }
      try {
        canvas.beginStroke({
          kind: 'erase',
          layer: activeRaster.id,
          point,
          diameter: brush.diameter,
        })
      } catch (error) {
        gesture.current = null
        receiveError(errorMessage(error))
      }
      return
    }
    if (tool === 'remove') {
      strokeUpdates.clear()
      gesture.current = {
        kind: 'inpaint',
        pointer,
        revision: canvasRevision,
        layer: null,
        points: [point],
        diameter: brush.diameter,
      }
      try {
        canvas.beginStroke({
          kind: 'inpaint',
          layer: null,
          point,
          diameter: brush.diameter,
        })
      } catch (error) {
        gesture.current = null
        receiveError(errorMessage(error))
      }
      return
    }
    if (tool === 'color_picker') {
      void canvas
        .sampleColor(physical)
        .then((color) => {
          const hex = rgbaToHex(color)
          if (!colorSampling?.complete(hex)) setBrush({ ...brush, color: hex })
        })
        .catch((error: unknown) => receiveError(errorMessage(error)))
    }
  }

  const armTouch = (pointer: number, clientX: number, clientY: number) => {
    if (pageFollowOffset.current !== 0 || pageAnimation.current) {
      pageAnimation.current?.cancel()
      pageAnimation.current = null
      clearPageFollow()
    }
    clearLongPress()
    pendingTouch.current = {
      pointer,
      start: clientPagePoint(clientX, clientY),
      physical: clientPhysicalPoint(clientX, clientY),
      clientX,
      clientY,
    }
    longPress.current = window.setTimeout(() => {
      longPress.current = null
      promoteLongPress()
    }, LONG_PRESS_MS)
  }

  // Touch has no hover, so a long press is the explicit selection affordance: it selects the
  // layer under the finger and arms a move, while a plain tap only selects.
  const promoteLongPress = () => {
    const pending = pendingTouch.current
    if (!pending || !page || tool !== 'select') return
    pendingTouch.current = null
    const target = hitTestLayers(page.layers, pending.start, layerFrames)
    if (!target) {
      selectLayers([])
      return
    }
    selectLayers([target.id])
    const originals = framesFor([target.id])
    if (!originals.length) return
    gesture.current = { kind: 'move', pointer: pending.pointer, start: pending.start, originals }
    beginTransform(originals)
  }

  const tapGesture = (pending: PendingTouch) => {
    if (!page || !canvas) return
    if (tool === 'select') {
      const target = hitTestLayers(page.layers, pending.start, layerFrames)
      if (target) {
        selectLayers([target.id])
        return
      }
      if (!navigatePage(1)) selectLayers([])
      return
    }
    if (tool === 'text') {
      void call(commands.addPointText, pending.start)
        .then((result) => {
          selectLayers([result.layer])
          return refresh(projectKey, pagesKey, pageKey)
        })
        .catch(() => undefined)
      return
    }
    if (tool === 'color_picker') {
      void canvas
        .sampleColor(pending.physical)
        .then((color) => {
          const hex = rgbaToHex(color)
          if (!colorSampling?.complete(hex)) setBrush({ ...brush, color: hex })
        })
        .catch((error: unknown) => receiveError(errorMessage(error)))
    }
  }

  const beginPinch = () => {
    const entries = [...pointers.current.entries()]
    cancelGesture()
    const bounds = surface.current?.getBoundingClientRect()
    if (entries.length < 2 || !bounds) return
    for (const [id, position] of entries) pointers.current.set(id, position)
    const [[firstId, first], [secondId, second]] = entries
    const dpr = window.devicePixelRatio
    const camera = useKoharuStore.getState().camera
    gesture.current = {
      kind: 'pinch',
      pointers: [firstId, secondId],
      startDistance: Math.max(
        1,
        Math.hypot(first.clientX - second.clientX, first.clientY - second.clientY),
      ),
      startZoom: camera.zoom,
      startMidpoint: {
        x: ((first.clientX + second.clientX) * 0.5 - bounds.x) * dpr,
        y: ((first.clientY + second.clientY) * 0.5 - bounds.y) * dpr,
      },
      startTranslation: camera.translation,
    }
  }

  const updatePinch = () => {
    const current = gesture.current
    if (current?.kind !== 'pinch' || !page) return
    const bounds = surface.current?.getBoundingClientRect()
    const first = pointers.current.get(current.pointers[0])
    const second = pointers.current.get(current.pointers[1])
    if (!bounds || !first || !second) return
    const dpr = window.devicePixelRatio
    const midpoint = {
      x: ((first.clientX + second.clientX) * 0.5 - bounds.x) * dpr,
      y: ((first.clientY + second.clientY) * 0.5 - bounds.y) * dpr,
    }
    const distance = Math.max(
      1,
      Math.hypot(first.clientX - second.clientX, first.clientY - second.clientY),
    )
    const zoom = clamp(current.startZoom * (distance / current.startDistance), 0.02, 16)
    const pageX = (current.startMidpoint.x - current.startTranslation[0]) / current.startZoom
    const pageY = (current.startMidpoint.y - current.startTranslation[1]) / current.startZoom
    const translation = clampCameraTranslation(
      [midpoint.x - pageX * zoom, midpoint.y - pageY * zoom],
      zoom,
      pageWidth ?? page.size.width,
      pageHeight ?? page.size.height,
      bounds.width * dpr,
      bounds.height * dpr,
      dpr,
    )
    useKoharuStore.setState({ camera: { zoom, translation, fitted: false } })
  }

  return (
    <main className='relative flex h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-tl-2xl bg-[var(--surface-canvas)]'>
      <CanvasCommandBar />
      <div className='relative flex min-h-0 min-w-0 flex-1'>
        <ToolBar />
        <div
          ref={surface}
          tabIndex={0}
          aria-label={t('canvas.surface')}
          aria-busy={page ? canvasState.status !== 'ready' : undefined}
          className='relative min-h-0 min-w-0 flex-1 touch-none overflow-hidden bg-[var(--surface-canvas)] outline-none'
          style={{
            cursor: page && canvasState.status === 'ready' ? canvasCursors[tool] : undefined,
          }}
          onContextMenu={(event) => event.preventDefault()}
          onPointerDown={(event) => {
            if (
              !page ||
              !canvas ||
              canvasState.status !== 'ready' ||
              canvasRevision === null ||
              commitPending.current ||
              event.button > 1
            )
              return
            if (event.target instanceof Element && event.target.closest('[data-canvas-control]'))
              return
            event.currentTarget.focus()
            event.currentTarget.setPointerCapture(event.pointerId)

            // Touch is deferred behind the slop threshold and a pinch can preempt it. Mouse and
            // pen keep the exact previous behavior.
            if (event.pointerType === 'touch') {
              const prior = pointers.current.size
              pointers.current.set(event.pointerId, {
                clientX: event.clientX,
                clientY: event.clientY,
              })
              if (prior === 0) {
                if (gesture.current) {
                  pointers.current.delete(event.pointerId)
                  return
                }
                armTouch(event.pointerId, event.clientX, event.clientY)
              } else if (prior === 1) {
                beginPinch()
              }
              event.preventDefault()
              return
            }

            const physical = clientPhysicalPoint(event.clientX, event.clientY)
            setCursor(physical)
            beginGesture(
              event.pointerId,
              clientPagePoint(event.clientX, event.clientY),
              physical,
              event.shiftKey || event.ctrlKey || event.metaKey,
              event.button === 1,
            )
            event.preventDefault()
          }}
          onPointerMove={(event) => {
            if (!page) return
            if (event.pointerType === 'touch') {
              if (!pointers.current.has(event.pointerId)) return
              pointers.current.set(event.pointerId, {
                clientX: event.clientX,
                clientY: event.clientY,
              })
              if (gesture.current?.kind === 'pinch') {
                updatePinch()
                return
              }
              const pending = pendingTouch.current
              if (pending && pending.pointer === event.pointerId) {
                const dx = event.clientX - pending.clientX
                const dy = event.clientY - pending.clientY
                if (Math.hypot(dx, dy) > TOUCH_SLOP) {
                  clearLongPress()
                  pendingTouch.current = null
                  const emptySelect =
                    tool === 'select' && !hitTestLayers(page.layers, pending.start, layerFrames)
                  if (emptySelect) {
                    gesture.current = {
                      kind: 'page',
                      pointer: pending.pointer,
                      startClientX: pending.clientX,
                      startClientY: pending.clientY,
                    }
                    followPage(dx)
                  } else {
                    beginGesture(pending.pointer, pending.start, pending.physical, false, false)
                    moveGesture(event.pointerId, [
                      { clientX: event.clientX, clientY: event.clientY },
                    ])
                  }
                }
                return
              }
            }
            const coalesced = event.nativeEvent.getCoalescedEvents?.() ?? [event.nativeEvent]
            moveGesture(event.pointerId, coalesced.length ? coalesced : [event.nativeEvent])
          }}
          onPointerUp={(event) => {
            const release = () => {
              if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                event.currentTarget.releasePointerCapture(event.pointerId)
              }
            }
            if (event.pointerType === 'touch') {
              pointers.current.delete(event.pointerId)
              if (gesture.current?.kind === 'pinch') {
                if (pointers.current.size < 2) gesture.current = null
                release()
                return
              }
              const pending = pendingTouch.current
              if (pending && pending.pointer === event.pointerId) {
                pendingTouch.current = null
                clearLongPress()
                tapGesture(pending)
                release()
                return
              }
              const current = gesture.current
              if (current?.kind === 'page') {
                gesture.current = null
                const dx = event.clientX - current.startClientX
                const dy = event.clientY - current.startClientY
                if (Math.abs(dx) >= PAGE_SWIPE_DISTANCE && Math.abs(dx) > Math.abs(dy) * 1.5) {
                  if (navigatePage(dx < 0 ? 1 : -1)) {
                    pageEntryOffset.current = pageFollowOffset.current
                  } else {
                    settlePageFollow()
                  }
                } else {
                  settlePageFollow()
                }
                release()
                return
              }
            }
            moveGesture(event.pointerId, [event.nativeEvent])
            finishGesture()
            release()
          }}
          onPointerCancel={() => cancelGesture()}
          onPointerLeave={(event) => {
            if (!event.currentTarget.hasPointerCapture(event.pointerId)) {
              setHovered(null)
              setCursor(null)
            }
          }}
          onWheel={(event) => {
            if (!page) return
            event.preventDefault()
            if (event.altKey && isBrushTool(tool)) {
              if (event.deltaY !== 0) {
                const currentBrush = useKoharuStore.getState().brush
                const delta = event.deltaY < 0 ? BRUSH_DIAMETER_STEP : -BRUSH_DIAMETER_STEP
                const nextDiameter = clamp(
                  currentBrush.diameter + delta,
                  MIN_BRUSH_DIAMETER,
                  maxBrushDiameter(page.size),
                )
                if (nextDiameter !== currentBrush.diameter) {
                  setBrush({ ...currentBrush, diameter: nextDiameter })
                }
              }
              return
            }
            const point = clientPhysicalPoint(event.clientX, event.clientY)
            const current = useKoharuStore.getState().camera
            let zoom = current.zoom
            let translation = current.translation
            if (event.ctrlKey) {
              zoom = clamp(current.zoom * Math.exp(-event.deltaY * 0.0015), 0.02, 16)
              const pageX = (point.x - current.translation[0]) / current.zoom
              const pageY = (point.y - current.translation[1]) / current.zoom
              translation = [point.x - pageX * zoom, point.y - pageY * zoom]
            } else {
              const dpr = window.devicePixelRatio
              let deltaX = event.deltaX
              let deltaY = event.deltaY
              if (event.shiftKey && deltaX === 0) {
                deltaX = deltaY
                deltaY = 0
              }
              translation = [
                current.translation[0] - deltaX * dpr,
                current.translation[1] - deltaY * dpr,
              ]
            }
            const bounds = event.currentTarget.getBoundingClientRect()
            const dpr = window.devicePixelRatio
            translation = clampCameraTranslation(
              translation,
              zoom,
              pageWidth ?? page.size.width,
              pageHeight ?? page.size.height,
              bounds.width * dpr,
              bounds.height * dpr,
              dpr,
            )
            useKoharuStore.setState({ camera: { zoom, translation, fitted: false } })
          }}
        >
          <div ref={pageContent} data-testid='canvas-page-content' className='absolute inset-0'>
            <canvas
              ref={setCanvasElement}
              data-testid='webgpu-canvas'
              aria-hidden
              className='pointer-events-none absolute inset-0 block size-full'
            />
            {page && canvasState.status === 'ready' && (
              <CanvasOverlay
                page={page}
                selected={selected}
                hovered={hovered}
                frames={layerFrames}
                previews={previews}
                draft={draft}
                cursor={cursor}
                brushSize={brush.diameter}
                showBrushCursor={isBrushTool(tool)}
                onTransformStart={beginTransform}
                onTransformFrame={updateTransform}
                onTransformEnd={finishTransform}
              />
            )}
          </div>
          {page && canvasState.status === 'error' && (
            <div
              role='alert'
              className={
                canvasState.hasFrame
                  ? 'absolute inset-x-0 top-3 z-20 flex justify-center px-3'
                  : 'absolute inset-0 z-20 grid place-items-center bg-[var(--surface-canvas)]/90 p-6'
              }
            >
              <div
                className={
                  canvasState.hasFrame
                    ? 'flex max-w-sm flex-col items-center gap-2 rounded-md border border-border bg-popover/95 p-3 text-center shadow-sm'
                    : 'flex max-w-sm flex-col items-center gap-2 text-center'
                }
              >
                <p className='text-sm font-medium text-foreground'>{t('canvas.unavailable')}</p>
                {canvasState.error && (
                  <p className='text-xs leading-relaxed text-muted-foreground'>
                    {canvasState.error.message}
                  </p>
                )}
                <Button
                  data-canvas-control
                  size='sm'
                  variant='outline'
                  className='mt-1'
                  onClick={canvasState.retry}
                >
                  {t('errors.tryAgain')}
                </Button>
              </div>
            </div>
          )}
          {!page && (
            <div className='pointer-events-none absolute inset-0 grid place-items-center'>
              <p className='text-[12px] text-muted-foreground'>{t('canvas.empty')}</p>
            </div>
          )}
        </div>
      </div>
      <StatusBar onZoomChange={setZoom} />
    </main>
  )
}

function hexToRgba(hex: string): [number, number, number, number] {
  const value = hex.replace('#', '')
  return [
    Number.parseInt(value.slice(0, 2), 16),
    Number.parseInt(value.slice(2, 4), 16),
    Number.parseInt(value.slice(4, 6), 16),
    255,
  ]
}

function rgbaToHex(color: [number, number, number, number]): string {
  return `#${color
    .slice(0, 3)
    .map((channel) => channel.toString(16).padStart(2, '0'))
    .join('')}`.toUpperCase()
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value))
}

function containCamera(
  viewportWidth: number,
  viewportHeight: number,
  pageWidth: number,
  pageHeight: number,
): CanvasView {
  if (viewportWidth <= 0 || viewportHeight <= 0 || pageWidth <= 0 || pageHeight <= 0) {
    return { zoom: 1, translation: [0, 0] }
  }
  const zoom = Math.max(
    Number.EPSILON,
    Math.min(viewportWidth / pageWidth, viewportHeight / pageHeight),
  )
  return {
    zoom,
    translation: [
      (viewportWidth - pageWidth * zoom) * 0.5,
      (viewportHeight - pageHeight * zoom) * 0.5,
    ],
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  return typeof error === 'string' ? error : 'The WebGPU canvas returned an unknown error.'
}

function clampCameraTranslation(
  translation: [number, number],
  zoom: number,
  pageWidth: number,
  pageHeight: number,
  viewportWidth: number,
  viewportHeight: number,
  dpr: number,
): [number, number] {
  const minOverlap = 100 * dpr
  const minX = minOverlap - pageWidth * zoom
  const minY = minOverlap - pageHeight * zoom
  const maxX = viewportWidth - minOverlap
  const maxY = viewportHeight - minOverlap

  const x =
    minX <= maxX ? clamp(translation[0], minX, maxX) : (viewportWidth - pageWidth * zoom) * 0.5
  const y =
    minY <= maxY ? clamp(translation[1], minY, maxY) : (viewportHeight - pageHeight * zoom) * 0.5
  return [x, y]
}

function mergeStrokeUpdates(current: StrokeUpdate, next: StrokeUpdate): StrokeUpdate {
  if (current.kind !== next.kind) return next
  current.points.push(...next.points)
  return current
}

function useFrameCommand<Value>(
  execute: (value: Value) => void | Promise<unknown>,
  merge?: (current: Value, next: Value) => Value,
): FrameCommand<Value> {
  const executeRef = useRef(execute)
  executeRef.current = execute
  const command = useRef<FrameCommand<Value> | null>(null)
  command.current ??= new FrameCommand((value) => executeRef.current(value), merge)

  useEffect(() => () => command.current?.clear(), [])
  return command.current
}

class FrameCommand<Value> {
  private pending: Value | undefined
  private frame: number | null = null

  constructor(
    private readonly execute: (value: Value) => void | Promise<unknown>,
    private readonly merge: (current: Value, next: Value) => Value = (_current, next) => next,
  ) {}

  schedule(value: Value): void {
    this.pending = this.pending === undefined ? value : this.merge(this.pending, value)
    if (this.frame !== null) return
    this.frame = requestAnimationFrame(() => {
      this.frame = null
      this.executePending()
    })
  }

  commit(): void {
    if (this.frame !== null) {
      cancelAnimationFrame(this.frame)
      this.frame = null
    }
    this.executePending()
  }

  clear(): void {
    if (this.frame !== null) cancelAnimationFrame(this.frame)
    this.frame = null
    this.pending = undefined
  }

  private executePending(): void {
    const value = this.pending
    if (value === undefined) return
    this.pending = undefined
    try {
      void Promise.resolve(this.execute(value)).catch((error: unknown) =>
        receiveError(errorMessage(error)),
      )
    } catch (error) {
      receiveError(errorMessage(error))
    }
  }
}
