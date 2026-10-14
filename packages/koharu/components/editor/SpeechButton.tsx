'use client'

import { Loader2, Play, Square } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { toggleSpeech, useSpeech } from '@/lib/speech'
import type { EntityId } from '@koharu/bridge/protocol'
import { Button } from '@koharu/ui/components/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@koharu/ui/components/tooltip'
import { cn } from '@koharu/ui/lib/utils'

export function SpeechButton({
  layer,
  disabled,
  className,
}: {
  layer: EntityId
  disabled?: boolean
  className?: string
}) {
  const { t } = useTranslation()
  const status = useSpeech((state) => (state.layer === layer ? state.status : null))
  const label =
    status === 'loading'
      ? t('speech.voicing')
      : status === 'playing'
        ? t('speech.stop')
        : t('speech.play')

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            type='button'
            variant='ghost'
            size='icon-xs'
            aria-label={status ? t('speech.stop') : t('speech.play')}
            aria-busy={status === 'loading'}
            data-testid={`speak-${layer}`}
            disabled={disabled}
            className={cn(
              'size-5 rounded-md text-muted-foreground hover:text-foreground',
              className,
            )}
            onClick={() => toggleSpeech(layer)}
          />
        }
      >
        {status === 'loading' ? (
          <Loader2 className='size-3 animate-spin motion-reduce:animate-none' />
        ) : status === 'playing' ? (
          <Square className='size-3' />
        ) : (
          <Play className='size-3' />
        )}
      </TooltipTrigger>
      <TooltipContent side='left'>{label}</TooltipContent>
    </Tooltip>
  )
}
