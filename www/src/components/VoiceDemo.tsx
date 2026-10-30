import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { MicIcon } from './icons'

type Phrase = { said: string; keyword: string; command: string }

const BARS = 18
const PHRASE_MS = 3600

/** Animated "listening → heard → matched → runs" loop. */
export function VoiceDemo() {
  const { t } = useTranslation()
  const phrases = t('demo.phrases', { returnObjects: true }) as Phrase[]
  const [index, setIndex] = useState(0)

  useEffect(() => {
    const id = setInterval(() => setIndex((i) => i + 1), PHRASE_MS)
    return () => clearInterval(id)
  }, [])

  const phrase = phrases[index % phrases.length]

  return (
    <div className="mockup-window border border-base-300 bg-base-200 shadow-2xl">
      <div className="space-y-4 border-t border-base-300 bg-base-100 p-5 font-mono text-sm">
        <div className="flex items-center gap-3">
          <span className="relative flex size-9 items-center justify-center rounded-full bg-primary text-primary-content">
            <motion.span
              className="absolute inset-0 rounded-full bg-primary"
              animate={{ scale: [1, 1.6], opacity: [0.5, 0] }}
              transition={{ duration: 1.4, repeat: Infinity, ease: 'easeOut' }}
            />
            <MicIcon className="relative size-5" />
          </span>
          <span className="text-success">● {t('demo.listening')}</span>
          <div className="ml-auto flex h-8 items-center gap-0.5">
            {Array.from({ length: BARS }, (_, i) => (
              <motion.span
                key={i}
                className="w-1 rounded-full bg-secondary"
                animate={{ height: ['20%', `${30 + ((i * 37) % 70)}%`, '20%'] }}
                transition={{ duration: 0.9, repeat: Infinity, delay: (i % 6) * 0.08, ease: 'easeInOut' }}
              />
            ))}
          </div>
        </div>

        <AnimatePresence mode="wait">
          <motion.div
            key={index}
            className="space-y-2"
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.35 }}
          >
            <Line label={t('demo.heard')} delay={0.1}>
              <span className="text-base-content">“{phrase.said}”</span>
            </Line>
            <Line label={t('demo.matched')} delay={0.7}>
              <span className="badge badge-primary badge-soft">{phrase.keyword}</span>
            </Line>
            <Line label={t('demo.runs')} delay={1.3}>
              <span className="break-all text-accent">$ {phrase.command}</span>
            </Line>
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  )
}

function Line({ label, delay, children }: { label: string; delay: number; children: React.ReactNode }) {
  return (
    <motion.div
      className="flex items-baseline gap-3"
      initial={{ opacity: 0, x: -6 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ delay, duration: 0.3 }}
    >
      <span className="w-20 shrink-0 text-right text-xs uppercase tracking-wide text-base-content/50">{label}</span>
      {children}
    </motion.div>
  )
}
