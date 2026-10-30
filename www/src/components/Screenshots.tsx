import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { screenshot } from '../assets'
import { Reveal, SectionTitle } from './Reveal'

/** Files in docs/screenshots (scripts/screenshots.sh), in the order of `screenshots.items`. */
const FILES = [
  '01-commands.png',
  '02-commands-light.png',
  '03-editor.png',
  '04-listen.png',
  '05-settings.png',
  '06-models.png',
  '07-launchers.png',
]
const WIDTH = 2304
const HEIGHT = 1440

type Item = { tab: string; title: string }

/** Tabs + a window preview; clicking it opens a modal carousel (arrows, keyboard, counter). */
export function Screenshots() {
  const { t } = useTranslation()
  const items = t('screenshots.items', { returnObjects: true }) as Item[]
  const shots = FILES.map((file, i) => ({ src: screenshot(file), ...items[i] }))
  const [index, setIndex] = useState(0)
  const [open, setOpen] = useState(false)
  const dialog = useRef<HTMLDialogElement>(null)
  const shot = shots[index]
  const step = (delta: number) => setIndex((i) => (i + delta + shots.length) % shots.length)

  useEffect(() => {
    const d = dialog.current
    if (!d) return
    if (open && !d.open) d.showModal()
    if (!open && d.open) d.close()
  }, [open])

  return (
    <section id="screenshots" className="scroll-mt-20 px-4 py-24">
      <SectionTitle title={t('screenshots.title')} subtitle={t('screenshots.subtitle')} />
      <Reveal className="mx-auto w-full max-w-5xl">
        <div role="tablist" aria-label={t('screenshots.title')} className="mb-4 flex flex-wrap justify-center gap-2">
          {shots.map((s, i) => (
            <button
              key={s.src}
              type="button"
              role="tab"
              aria-selected={i === index}
              className={`btn btn-sm ${i === index ? 'btn-primary btn-soft' : 'btn-ghost'}`}
              onClick={() => setIndex(i)}
            >
              {s.tab}
            </button>
          ))}
        </div>

        <div className="overflow-hidden rounded-box border border-base-300 bg-base-200 shadow-2xl">
          <div className="flex items-center gap-2 border-b border-base-300 px-4 py-3">
            <span className="size-3 rounded-full bg-error/70" />
            <span className="size-3 rounded-full bg-warning/70" />
            <span className="size-3 rounded-full bg-success/70" />
            <span className="ml-2 truncate text-xs text-base-content/60">{shot.title}</span>
          </div>
          <button
            type="button"
            className="block aspect-[16/10] w-full cursor-zoom-in overflow-hidden bg-neutral"
            onClick={() => setOpen(true)}
            aria-label={`${t('screenshots.open')}: ${shot.title}`}
          >
            <AnimatePresence mode="wait">
              <motion.img
                key={shot.src}
                src={shot.src}
                alt={`Dicturs — ${shot.title}`}
                width={WIDTH}
                height={HEIGHT}
                loading="lazy"
                className="block h-full w-full object-contain"
                initial={{ opacity: 0, scale: 0.985 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.985 }}
                transition={{ duration: 0.22 }}
              />
            </AnimatePresence>
          </button>
        </div>
      </Reveal>

      <dialog
        ref={dialog}
        className="modal"
        aria-label={t('screenshots.title')}
        onClose={() => setOpen(false)}
        onKeyDown={(e) => {
          if (e.key === 'ArrowLeft') step(-1)
          if (e.key === 'ArrowRight') step(1)
        }}
      >
        <div className="modal-box flex max-h-[94vh] w-[min(96vw,1280px)] max-w-none flex-col p-0">
          <div className="flex items-center justify-between gap-4 border-b border-base-300 px-5 py-4">
            <div>
              <h3 className="font-semibold">{shot.title}</h3>
              <p className="mt-1 text-sm text-base-content/60">
                {index + 1} / {shots.length}
              </p>
            </div>
            <button type="button" className="btn btn-circle btn-ghost btn-sm" aria-label={t('screenshots.close')} onClick={() => setOpen(false)}>
              ✕
            </button>
          </div>

          <div className="relative h-[min(68vh,820px)] min-h-0 bg-neutral">
            <button
              type="button"
              className="btn btn-circle absolute left-3 top-1/2 z-10 -translate-y-1/2 border-white/20 bg-black/60 text-2xl text-white hover:bg-primary"
              aria-label={t('screenshots.previous')}
              onClick={() => step(-1)}
            >
              ‹
            </button>
            <AnimatePresence mode="wait">
              <motion.img
                key={shot.src}
                src={shot.src}
                alt={`Dicturs — ${shot.title}`}
                width={WIDTH}
                height={HEIGHT}
                className="h-full w-full object-contain"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.18 }}
              />
            </AnimatePresence>
            <button
              type="button"
              className="btn btn-circle absolute right-3 top-1/2 z-10 -translate-y-1/2 border-white/20 bg-black/60 text-2xl text-white hover:bg-primary"
              aria-label={t('screenshots.next')}
              onClick={() => step(1)}
            >
              ›
            </button>
          </div>

          <div className="flex items-center justify-between gap-4 border-t border-base-300 px-5 py-4">
            <span className="text-sm text-base-content/60">{shot.tab}</span>
            <div className="flex gap-2">
              <button type="button" className="btn btn-outline btn-sm" onClick={() => step(-1)}>
                {t('screenshots.previous')}
              </button>
              <button type="button" className="btn btn-primary btn-sm" onClick={() => step(1)}>
                {t('screenshots.next')}
              </button>
            </div>
          </div>
        </div>
        {/* Clicking outside the box closes the modal. */}
        <form method="dialog" className="modal-backdrop backdrop-blur-sm">
          <button aria-label={t('screenshots.close')}>close</button>
        </form>
      </dialog>
    </section>
  )
}
