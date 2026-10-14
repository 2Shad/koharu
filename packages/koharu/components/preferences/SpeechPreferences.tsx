'use client'

import { Check, Download, Loader2, Plus, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  NumberField,
  PreferencePage,
  PreferenceRow,
  PreferenceSection,
  TextField,
} from '@/components/preferences/PreferenceFields'
import { call } from '@/lib/backend'
import { installSpeech, useSpeech } from '@/lib/speech'
import { useKoharuStore } from '@/lib/store'
import {
  commands,
  type CharacterVoice,
  type DialogueConfig,
  type SpeechConfig,
  type SpeechModel,
} from '@koharu/bridge/protocol'
import { Button } from '@koharu/ui/components/button'
import { Input } from '@koharu/ui/components/input'
import { Textarea } from '@koharu/ui/components/textarea'

export function SpeechPreferences({
  value,
  dialogue,
  onChange,
  onDialogueChange,
}: {
  value: SpeechConfig
  dialogue: DialogueConfig
  onChange: (value: SpeechConfig) => void
  onDialogueChange: (value: DialogueConfig) => void
}) {
  const { t } = useTranslation()
  const voices = value.voices ?? []
  const update = (patch: Partial<SpeechConfig>) => onChange({ ...value, ...patch })
  const updateVoice = (index: number, patch: Partial<CharacterVoice>) =>
    update({
      voices: voices.map((voice, candidate) =>
        candidate === index ? { ...voice, ...patch } : voice,
      ),
    })

  return (
    <PreferencePage
      title={t('settings.speech.title')}
      description={t('settings.speech.description')}
    >
      <VoiceModel />

      <PreferenceSection
        title={t('settings.speech.runtime')}
        description={t('settings.speech.runtimeDescription')}
      >
        <PreferenceRow
          title={t('settings.speech.cpuExpertLayers')}
          description={t('settings.speech.cpuExpertLayersDescription')}
        >
          <NumberField
            label={t('settings.speech.layers')}
            value={value.cpu_expert_layers ?? null}
            min={0}
            max={64}
            step={1}
            onChange={(layers) => update({ cpu_expert_layers: layers })}
          />
        </PreferenceRow>
        <PreferenceRow
          title={t('settings.speech.maxTokens')}
          description={t('settings.speech.maxTokensDescription')}
        >
          <NumberField
            label={t('settings.speech.frames')}
            value={value.max_tokens ?? null}
            min={50}
            max={4000}
            step={50}
            onChange={(frames) => update({ max_tokens: frames ?? undefined })}
          />
        </PreferenceRow>
        <PreferenceRow title={t('settings.speech.threads')}>
          <NumberField
            label={t('settings.speech.threads')}
            value={value.threads ?? null}
            min={1}
            max={256}
            step={1}
            onChange={(threads) => update({ threads: threads ?? undefined })}
          />
        </PreferenceRow>
        <PreferenceRow
          title={t('settings.speech.server')}
          description={t('settings.speech.serverDescription')}
        >
          <TextField
            label={t('settings.speech.server')}
            value={value.server ?? ''}
            onChange={(server) => update({ server: server.trim() || null })}
          />
        </PreferenceRow>
        <PreferenceRow
          title={t('settings.speech.model')}
          description={t('settings.speech.modelDescription')}
        >
          <TextField
            label={t('settings.speech.model')}
            value={value.model ?? ''}
            onChange={(model) => update({ model: model.trim() || null })}
          />
        </PreferenceRow>
      </PreferenceSection>

      <PreferenceSection
        title={t('settings.speech.direction')}
        description={t('settings.speech.directionDescription')}
      >
        <PreferenceRow title={t('settings.speech.instructions')} align='start'>
          <Textarea
            aria-label={t('settings.speech.instructions')}
            value={dialogue.instructions ?? ''}
            placeholder={t('settings.speech.instructionsPlaceholder')}
            className='min-h-20 text-[12px]'
            onChange={(event) =>
              onDialogueChange({
                ...dialogue,
                instructions: event.currentTarget.value.trim() ? event.currentTarget.value : null,
              })
            }
          />
        </PreferenceRow>
      </PreferenceSection>

      <PreferenceSection
        title={t('settings.speech.voices')}
        description={t('settings.speech.voicesDescription')}
      >
        {voices.map((voice, index) => (
          <div key={index} className='grid grid-cols-[1fr_1.4fr_auto] items-start gap-2 py-3'>
            <Input
              aria-label={t('settings.speech.character')}
              placeholder={t('settings.speech.character')}
              value={voice.character}
              className='h-8 text-[12px]'
              onChange={(event) => updateVoice(index, { character: event.currentTarget.value })}
            />
            <div className='grid gap-1.5'>
              <Input
                aria-label={t('settings.speech.referenceAudio')}
                placeholder={t('settings.speech.referenceAudio')}
                value={voice.reference_audio}
                className='h-8 text-[12px]'
                onChange={(event) =>
                  updateVoice(index, { reference_audio: event.currentTarget.value })
                }
              />
              <Input
                aria-label={t('settings.speech.referenceText')}
                placeholder={t('settings.speech.referenceText')}
                value={voice.reference_text}
                className='h-8 text-[12px]'
                onChange={(event) =>
                  updateVoice(index, { reference_text: event.currentTarget.value })
                }
              />
            </div>
            <Button
              type='button'
              variant='ghost'
              size='icon-sm'
              aria-label={t('settings.speech.removeVoice', {
                character: voice.character.trim() || index + 1,
              })}
              onClick={() =>
                update({ voices: voices.filter((_, candidate) => candidate !== index) })
              }
            >
              <Trash2 className='size-3.5' />
            </Button>
          </div>
        ))}
        <div className='py-3'>
          <Button
            type='button'
            variant='outline'
            size='sm'
            className='gap-1.5 text-[12px]'
            onClick={() =>
              update({
                voices: [...voices, { character: '', reference_audio: '', reference_text: '' }],
              })
            }
          >
            <Plus className='size-3.5' /> {t('settings.speech.addVoice')}
          </Button>
        </div>
      </PreferenceSection>
    </PreferencePage>
  )
}

function VoiceModel() {
  const { t } = useTranslation()
  const savedModel = useKoharuStore((state) => state.preferences?.speech.model ?? null)
  const installing = useSpeech((state) => state.installing)
  const [model, setModel] = useState<SpeechModel | null>(null)

  useEffect(() => {
    let current = true
    void call(commands.getSpeechModel)
      .then((status) => {
        if (current) setModel(status)
      })
      .catch(() => undefined)
    return () => {
      current = false
    }
  }, [savedModel])

  const install = () =>
    void installSpeech()
      .then((status) => status && setModel(status))
      .catch(() => undefined)

  const gigabytes = model ? Math.round(model.download_bytes / 1e9) : null
  return (
    <PreferenceSection
      title={t('settings.speech.voiceModel')}
      description={t('settings.speech.voiceModelDescription')}
    >
      <PreferenceRow
        title={model?.ready ? t('settings.speech.modelReady') : t('settings.speech.modelMissing')}
        description={model?.local ? t('settings.speech.modelLocal') : undefined}
      >
        <div className='flex justify-end'>
          {model?.ready ? (
            <span className='flex items-center gap-1.5 text-[12px] text-muted-foreground'>
              <Check className='size-3.5' /> {t('settings.speech.installed')}
            </span>
          ) : (
            <Button
              type='button'
              size='sm'
              className='gap-1.5 text-[12px]'
              disabled={!model || model.local || installing}
              onClick={install}
            >
              {installing ? (
                <Loader2 className='size-3.5 animate-spin motion-reduce:animate-none' />
              ) : (
                <Download className='size-3.5' />
              )}
              {installing
                ? t('settings.speech.downloading')
                : t('settings.speech.download', { size: gigabytes ?? '…' })}
            </Button>
          )}
        </div>
      </PreferenceRow>
    </PreferenceSection>
  )
}
