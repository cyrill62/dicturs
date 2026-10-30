import { motion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { GithubIcon } from '../../src/components/icons'
import { Install } from '../../src/components/Install'
import { Reveal, SectionTitle } from '../../src/components/Reveal'
import { Screenshots } from '../../src/components/Screenshots'
import { VoiceDemo } from '../../src/components/VoiceDemo'
import { REPO_URL } from '../../src/site'

type Item = { icon?: string; title: string; text: string }

export function Page() {
  return (
    <>
      <Hero />
      <Screenshots />
      <Apps />
      <Features />
      <Steps />
      <Install />
      <Faq />
    </>
  )
}

function Hero() {
  const { t } = useTranslation()
  return (
    <section id="top" className="relative overflow-hidden px-4 pb-24 pt-16 md:pt-24">
      <div className="pointer-events-none absolute -top-40 left-1/2 size-[42rem] -translate-x-1/2 rounded-full bg-primary/20 blur-3xl" />
      <div className="relative mx-auto grid max-w-6xl items-center gap-12 lg:grid-cols-2">
        <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6 }}>
          <span className="badge badge-outline badge-primary mb-6">{t('hero.badge')}</span>
          <h1 className="text-5xl font-extrabold leading-tight md:text-7xl">
            {t('hero.title1')}
            <br />
            <span className="text-gradient">{t('hero.title2')}</span>
          </h1>
          <p className="mt-6 max-w-xl text-lg text-base-content/75">{t('hero.subtitle')}</p>
          <div className="mt-8 flex flex-wrap gap-3">
            <a href="#install" className="btn btn-primary btn-lg">
              {t('hero.install')}
            </a>
            <a href={REPO_URL} className="btn btn-ghost btn-lg gap-2">
              <GithubIcon className="size-5" />
              {t('hero.github')}
            </a>
          </div>
        </motion.div>
        <motion.div initial={{ opacity: 0, scale: 0.95 }} animate={{ opacity: 1, scale: 1 }} transition={{ duration: 0.6, delay: 0.15 }}>
          <VoiceDemo />
        </motion.div>
      </div>
    </section>
  )
}

function Apps() {
  const { t } = useTranslation()
  const apps = [
    { key: 'cli', bin: 'dicturs', preview: <CliPreview /> },
    { key: 'tui', bin: 'dicturs-tui', preview: <TuiPreview /> },
    { key: 'gui', bin: 'dicturs-gui', preview: <GuiPreview /> },
  ] as const
  return (
    <section id="apps" className="scroll-mt-20 bg-base-200 px-4 py-24">
      <SectionTitle title={t('apps.title')} subtitle={t('apps.subtitle')} />
      <div className="mx-auto grid max-w-6xl gap-6 md:grid-cols-3">
        {apps.map(({ key, bin, preview }, i) => (
          <Reveal key={key} delay={i * 0.1} className="card border border-base-300 bg-base-100 shadow-lg">
            <figure className="h-44 bg-neutral p-4">{preview}</figure>
            <div className="card-body">
              <h3 className="card-title">
                {t(`apps.${key}.name`)}
                <code className="badge badge-soft badge-secondary font-mono">{bin}</code>
              </h3>
              <p className="text-base-content/75">{t(`apps.${key}.text`)}</p>
            </div>
          </Reveal>
        ))}
      </div>
    </section>
  )
}

function CliPreview() {
  return (
    <pre className="h-full w-full overflow-hidden font-mono text-xs leading-relaxed text-neutral-content/90">
      <span className="text-accent">$</span> dicturs add "open browser" "firefox"{'\n'}
      added #12{'\n'}
      <span className="text-accent">$</span> dicturs say "open browser" --dry-run{'\n'}
      ⇒ open browser (100%): firefox{'\n'}
      <span className="text-accent">$</span> dicturs listen
    </pre>
  )
}

function TuiPreview() {
  const rows = [
    ['●', 'open browser', 'Apps/Web'],
    ['●', 'volume up', 'System/Audio'],
    ['○', 'take screenshot', 'Desktop'],
    ['●', 'stop listening', 'Dicturs'],
  ]
  return (
    <div className="h-full w-full overflow-hidden rounded border border-secondary/60 p-2 font-mono text-xs text-neutral-content/90">
      <div className="mb-1 text-secondary">─ Commands ─ Listen ─ Settings ─</div>
      {rows.map(([dot, kw, cat], i) => (
        <div key={kw} className={`flex gap-2 ${i === 1 ? 'bg-primary/40' : ''}`}>
          <span className={dot === '●' ? 'text-success' : 'text-neutral-content/40'}>{dot}</span>
          <span className="flex-1">{kw}</span>
          <span className="text-neutral-content/50">{cat}</span>
        </div>
      ))}
    </div>
  )
}

function GuiPreview() {
  return (
    <div className="flex h-full w-full overflow-hidden rounded-lg bg-base-100 text-base-content">
      <div className="w-1/4 space-y-1 bg-base-200 p-2">
        <img src={`${import.meta.env.BASE_URL}logo.svg`} alt="" className="mb-2 size-5" />
        {[1, 2, 3].map((i) => (
          <div key={i} className={`h-2 rounded ${i === 1 ? 'bg-primary' : 'bg-base-300'}`} />
        ))}
      </div>
      <div className="flex-1 space-y-2 p-3">
        <div className="flex gap-2">
          <div className="h-4 w-20 rounded bg-primary" />
          <div className="h-4 flex-1 rounded bg-base-300" />
        </div>
        {[0, 1, 2].map((i) => (
          <div key={i} className="flex items-center gap-2">
            <input type="checkbox" className="toggle toggle-xs toggle-primary" defaultChecked={i !== 2} tabIndex={-1} aria-hidden="true" />
            <div className="h-2 flex-1 rounded bg-base-300" />
          </div>
        ))}
      </div>
    </div>
  )
}

function Features() {
  const { t } = useTranslation()
  const items = t('features.items', { returnObjects: true }) as Item[]
  return (
    <section id="features" className="scroll-mt-20 px-4 py-24">
      <SectionTitle title={t('features.title')} />
      <div className="mx-auto grid max-w-6xl gap-6 sm:grid-cols-2 lg:grid-cols-4">
        {items.map((item, i) => (
          <Reveal key={item.title} delay={(i % 4) * 0.08} className="card border border-base-300 bg-base-100 transition hover:-translate-y-1 hover:shadow-xl">
            <div className="card-body">
              <span className="text-3xl" aria-hidden="true">
                {item.icon}
              </span>
              <h3 className="card-title text-lg">{item.title}</h3>
              <p className="text-base-content/75">{item.text}</p>
            </div>
          </Reveal>
        ))}
      </div>
    </section>
  )
}

function Steps() {
  const { t } = useTranslation()
  const items = t('steps.items', { returnObjects: true }) as Item[]
  return (
    <section className="px-4 pb-24">
      <SectionTitle title={t('steps.title')} />
      <ol className="mx-auto grid max-w-5xl gap-6 md:grid-cols-3">
        {items.map((item, i) => (
          <Reveal key={item.title} delay={i * 0.12} className="flex gap-4">
            <span className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary text-lg font-bold text-primary-content">
              {i + 1}
            </span>
            <div>
              <h3 className="text-lg font-semibold">{item.title}</h3>
              <p className="text-base-content/75">{item.text}</p>
            </div>
          </Reveal>
        ))}
      </ol>
    </section>
  )
}

function Faq() {
  const { t } = useTranslation()
  const items = t('faq.items', { returnObjects: true }) as { q: string; a: string }[]
  return (
    <section id="faq" className="scroll-mt-20 px-4 py-24">
      <SectionTitle title={t('faq.title')} />
      <Reveal className="mx-auto max-w-3xl space-y-3">
        {items.map((item, i) => (
          <div key={item.q} className="collapse-arrow collapse border border-base-300 bg-base-100">
            <input type="radio" name="faq" defaultChecked={i === 0} aria-label={item.q} />
            <div className="collapse-title font-semibold">{item.q}</div>
            <div className="collapse-content text-base-content/75">
              <p>{item.a}</p>
            </div>
          </div>
        ))}
      </Reveal>
    </section>
  )
}
