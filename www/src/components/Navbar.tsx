import { useTranslation } from 'react-i18next'
import { LANGUAGES, setLanguage, type Language } from '../i18n'
import { LOGO } from '../assets'
import { REPO_URL } from '../site'
import { GithubIcon } from './icons'

export function Navbar() {
  const { t, i18n } = useTranslation()
  const links = [
    ['#screenshots', t('nav.screenshots')],
    ['#apps', t('nav.apps')],
    ['#features', t('nav.features')],
    ['#install', t('nav.install')],
    ['#faq', t('nav.faq')],
  ] as const

  return (
    <header className="sticky top-0 z-50 border-b border-base-300/60 bg-base-100/80 backdrop-blur">
      <nav className="navbar mx-auto max-w-6xl px-4">
        <div className="navbar-start">
          <a href="#top" className="flex items-center gap-2 text-xl font-bold">
            <img src={LOGO} alt="" className="size-8" />
            <span>
              dictu<span className="text-gradient">rs</span>
            </span>
          </a>
        </div>
        <div className="navbar-center hidden md:flex">
          <ul className="menu menu-horizontal gap-1">
            {links.map(([href, label]) => (
              <li key={href}>
                <a href={href}>{label}</a>
              </li>
            ))}
          </ul>
        </div>
        <div className="navbar-end gap-2">
          <select
            className="select select-sm w-auto"
            aria-label="Language"
            value={i18n.resolvedLanguage}
            onChange={(e) => setLanguage(e.target.value as Language)}
          >
            {Object.entries(LANGUAGES).map(([code, label]) => (
              <option key={code} value={code}>
                {label}
              </option>
            ))}
          </select>
          <a href={REPO_URL} className="btn btn-ghost btn-sm btn-square" aria-label={t('nav.github')}>
            <GithubIcon className="size-5" />
          </a>
        </div>
      </nav>
    </header>
  )
}
