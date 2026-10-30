export function Head() {
  const base = import.meta.env.BASE_URL
  return (
    <>
      <link rel="icon" href={`${base}logo.svg`} type="image/svg+xml" />
      <meta name="theme-color" content="#7c3aed" />
      <meta property="og:title" content="Dicturs — say it, it runs" />
      <meta property="og:description" content="Offline voice commands for your desktop, written in Rust." />
    </>
  )
}
