/** Repository and site URL; VITE_REPO / VITE_SITE_URL override them at build time. */
export const REPO: string = import.meta.env.VITE_REPO || 'cyrill62/dicturs'
export const SITE_URL: string = (import.meta.env.VITE_SITE_URL || 'https://dicturs.app').replace(/\/$/, '')
export const REPO_URL = `https://github.com/${REPO}`
export const RELEASES_URL = `${REPO_URL}/releases/latest`
export const INSTALL_CMD = `curl -fsSL ${SITE_URL}/install.sh | sh`
