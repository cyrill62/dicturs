import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import vike from 'vike/plugin'
import { defineConfig } from 'vite'

export default defineConfig({
  // Served at the root of https://dicturs.app (GitHub Pages, custom domain). BASE_URL overrides
  // it, e.g. "/dicturs/" for a project site without the custom domain.
  base: process.env.BASE_URL ?? '/',
  plugins: [vike(), react(), tailwindcss()],
  // The logo and screenshots are imported from the repo root, so they stay in one place.
  server: { fs: { allow: ['..'] } },
})
