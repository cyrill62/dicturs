import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { INSTALL_CMD, RELEASES_URL, REPO_URL, SITE_URL } from '../site'
import { CopyIcon } from './icons'
import { Reveal, SectionTitle } from './Reveal'

type Tab = 'script' | 'cargo' | 'windows'

export function Install() {
  const { t } = useTranslation()
  const [tab, setTab] = useState<Tab>('script')

  return (
    <section id="install" className="scroll-mt-20 bg-base-200 px-4 py-24">
      <SectionTitle title={t('install.title')} subtitle={t('install.subtitle')} />
      <Reveal className="mx-auto max-w-3xl">
        <div role="tablist" className="tabs tabs-box mb-4 w-fit">
          {(['script', 'cargo', 'windows'] as const).map((id) => (
            <button key={id} role="tab" className={`tab ${tab === id ? 'tab-active' : ''}`} onClick={() => setTab(id)}>
              {t(`install.${id}`)}
            </button>
          ))}
        </div>

        <div className="card border border-base-300 bg-base-100 shadow-xl">
          <div className="card-body gap-4">
            {tab === 'script' && (
              <>
                <Command text={INSTALL_CMD} />
                <p className="text-base-content/70">{t('install.scriptNote')}</p>
                <Command text={`curl -fsSL ${SITE_URL}/install.sh | sh -s -- --bins cli,tui --dir ~/bin`} />
              </>
            )}
            {tab === 'cargo' && (
              <>
                <p className="text-base-content/70">{t('install.cargoNote')}</p>
                <Command text={`cargo install --git ${REPO_URL}`} />
                <Command text={`cargo install --git ${REPO_URL} --no-default-features --features whisper,cli,tui`} />
              </>
            )}
            {tab === 'windows' && (
              <>
                <p className="text-base-content/70">{t('install.windowsNote')}</p>
                <a className="btn btn-primary w-fit" href={RELEASES_URL}>
                  {t('install.releases')}
                </a>
              </>
            )}
            <div className="divider my-0" />
            <p className="text-base-content/70">{t('install.then')}</p>
            <Command text="dicturs model use base && dicturs-gui" />
          </div>
        </div>
      </Reveal>
    </section>
  )
}

function Command({ text }: { text: string }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    await navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }
  return (
    <div className="flex items-center gap-2 rounded-field bg-neutral p-3 font-mono text-sm text-neutral-content">
      <span className="select-none text-accent">$</span>
      <code className="flex-1 overflow-x-auto whitespace-nowrap">{text}</code>
      <button className="btn btn-ghost btn-xs gap-1 text-neutral-content" onClick={copy} aria-label={t('install.copy')}>
        <CopyIcon className="size-4" />
        {copied ? t('install.copied') : t('install.copy')}
      </button>
    </div>
  )
}
