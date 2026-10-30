// Files from the repository root, imported so Vite serves them under the site's base path.
import logoUrl from '../../assets/logo.svg'

export const LOGO = logoUrl

const shots = import.meta.glob<string>('../../docs/screenshots/*.png', { eager: true, import: 'default' })

/** URL of a screenshot of docs/screenshots (scripts/screenshots.sh), by file name. */
export function screenshot(file: string): string {
  const url = shots[`../../docs/screenshots/${file}`]
  if (!url) throw new Error(`missing screenshot docs/screenshots/${file}`)
  return url
}
