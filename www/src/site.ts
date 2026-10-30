/** Repository and site URL: set by the Pages workflow, defaults for local builds. */
export const REPO: string = import.meta.env.VITE_REPO || 'cyrill62/dicturs'
const [owner, name] = REPO.split('/')
export const SITE_URL: string = (import.meta.env.VITE_SITE_URL || `https://${owner}.github.io/${name}`).replace(/\/$/, '')
export const REPO_URL = `https://github.com/${REPO}`
export const RELEASES_URL = `${REPO_URL}/releases/latest`
export const INSTALL_CMD = `curl -fsSL ${SITE_URL}/install.sh | sh`
