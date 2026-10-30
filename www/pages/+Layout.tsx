import './tailwind.css'
import '../src/i18n'

import { MotionConfig } from 'motion/react'
import { useEffect, type ReactNode } from 'react'
import { Footer } from '../src/components/Footer'
import { Navbar } from '../src/components/Navbar'
import { detectLanguage, setLanguage } from '../src/i18n'

export function Layout({ children }: { children: ReactNode }) {
  useEffect(() => {
    const lang = detectLanguage()
    if (lang !== 'en') setLanguage(lang)
  }, [])

  return (
    <MotionConfig reducedMotion="user">
      <div className="min-h-screen bg-base-100 text-base-content">
        <Navbar />
        <main>{children}</main>
        <Footer />
      </div>
    </MotionConfig>
  )
}
