import { LOGO, screenshot } from '../src/assets'
import { SITE_URL } from '../src/site'

export function Head() {
  return (
    <>
      <link rel="icon" href={LOGO} type="image/svg+xml" />
      <link rel="canonical" href={`${SITE_URL}/`} />
      <meta name="theme-color" content="#7c3aed" />
      <meta property="og:type" content="website" />
      <meta property="og:url" content={`${SITE_URL}/`} />
      <meta property="og:title" content="Dicturs — say it, it runs" />
      <meta property="og:description" content="Offline voice commands for your desktop, written in Rust." />
      <meta property="og:image" content={`${SITE_URL}${screenshot('01-commands.png')}`} />
      <meta name="twitter:card" content="summary_large_image" />
    </>
  )
}
