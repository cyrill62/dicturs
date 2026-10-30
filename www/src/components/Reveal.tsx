import { motion } from 'motion/react'
import type { ReactNode } from 'react'

/** Fades its content in when it scrolls into view. */
export function Reveal({ children, delay = 0, className }: { children: ReactNode; delay?: number; className?: string }) {
  return (
    <motion.div
      className={className}
      initial={{ opacity: 0, y: 24 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: '-60px' }}
      transition={{ duration: 0.5, delay, ease: 'easeOut' }}
    >
      {children}
    </motion.div>
  )
}

export function SectionTitle({ title, subtitle }: { title: string; subtitle?: string }) {
  return (
    <Reveal className="mx-auto mb-12 max-w-2xl text-center">
      <h2 className="text-3xl font-bold md:text-4xl">{title}</h2>
      {subtitle && <p className="mt-3 text-lg text-base-content/70">{subtitle}</p>}
    </Reveal>
  )
}
