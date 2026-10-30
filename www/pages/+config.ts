import type { Config } from 'vike/types'
import vikeReact from 'vike-react/config'

export default {
  extends: [vikeReact],
  // Static HTML for GitHub Pages.
  prerender: true,
  title: 'Dicturs — run commands with your voice',
  description:
    'Dicturs runs your shell commands when you say a keyword. Offline speech recognition (whisper.cpp), CLI, terminal and desktop apps, wofi/rofi integration.',
  lang: 'en',
} satisfies Config
