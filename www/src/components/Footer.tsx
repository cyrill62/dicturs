import { useTranslation } from 'react-i18next'
import { LOGO } from '../assets'
import { RELEASES_URL, REPO_URL } from '../site'

export function Footer() {
  const { t } = useTranslation()
  return (
    <footer className="footer footer-center gap-3 border-t border-base-300 bg-base-200 p-10 text-base-content/70">
      <img src={LOGO} alt="Dicturs" className="size-10" />
      <p>{t('footer.made')}</p>
      <nav className="flex gap-4">
        <a className="link link-hover" href={REPO_URL}>
          GitHub
        </a>
        <a className="link link-hover" href={RELEASES_URL}>
          Releases
        </a>
        <a className="link link-hover" href={`${import.meta.env.BASE_URL}install.sh`}>
          install.sh
        </a>
      </nav>
    </footer>
  )
}
