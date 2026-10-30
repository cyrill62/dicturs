import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import vike from 'vike/plugin'
import { defineConfig } from 'vite'

export default defineConfig({
  // GitHub Pages serves the site under /<repo>/: the workflow passes it as BASE_URL.
  base: process.env.BASE_URL ?? '/',
  plugins: [vike(), react(), tailwindcss()],
})
